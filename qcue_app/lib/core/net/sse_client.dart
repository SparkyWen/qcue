// QCue S4-R20/R21/R23: the SSE client for the streaming surfaces (recall chat,
// wiki-query synthesis, Dream/ingest progress). The app-server serializes every
// frame as a [RuntimeEventEnvelope] in `data:` with the envelope `seq` carried in
// `id:`; this client:
//   - authenticates with the JWT as a `?token=` query param, NOT a header
//     (pitfall #15 — `EventSource` can't set `Authorization`);
//   - decodes each `data:` envelope and maps `{event, payload}` onto the sealed
//     [SseEvent] taxonomy (`sse_event.dart`);
//   - SKIPS unknown `event` kinds and malformed frames (forward-compat — never
//     throws into the UI stream);
//   - on a drop, REPLAYS from the last good seq via `?since_seq=<seq>` against
//     the server's 20-event replay ring, deduping any re-sent frames.
//
// The raw byte→frame parsing is abstracted behind [SseTransport] so the
// reconnect/dedup/skip logic is unit-testable without a socket; the production
// transport ([HttpSseTransport]) reads `text/event-stream` over `package:http`.
import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;

import '../models/runtime_event.dart';
import '../models/sse_event.dart';

/// One parsed SSE frame: the numeric `id:` (the envelope `seq`) + the raw
/// `data:` payload string (the JSON-encoded [RuntimeEventEnvelope]). The `event:`
/// line is intentionally ignored — the server leaves it unset and the real
/// discriminant lives INSIDE the envelope's `event` field.
class RawSseFrame {
  const RawSseFrame({required this.id, required this.data});
  final int id;
  final String data;
}

/// Raised by an [SseTransport] when the underlying connection drops mid-stream;
/// the [SseClient] catches it and reconnects with the replay offset.
class SseDropped implements Exception {
  const SseDropped();
}

/// Abstracts the raw frame source so reconnect/replay/skip are testable without
/// a real socket. [connect] is called once per (re)connection with the fully
/// built URL (already carrying `?token=` and, on reconnect, `&since_seq=`).
abstract interface class SseTransport {
  Stream<RawSseFrame> connect(String url);
}

/// SSE client: `?token=` auth, replay-on-reconnect via the last seq, unknown +
/// malformed frame skip. [stream] yields the sealed [SseEvent] taxonomy.
class SseClient {
  // The `token` callback is read on every (re)connect so a rotated JWT is
  // picked up without re-subscribing.
  SseClient(
    this._transport, {
    required String Function() token,
    Future<bool> Function()? onUnauthorized,
  })
      // ignore: prefer_initializing_formals
      : _token = token,
        // ignore: prefer_initializing_formals
        _onUnauthorized = onUnauthorized;

  final SseTransport _transport;
  final String Function() _token;

  /// AUTH-R4: invoked once on a setup-401 to refresh the bearer; if it returns
  /// true the stream reconnects with the new `?token=`. A second consecutive 401
  /// surfaces as an error (no loop). Wired to the same single-flight refresh.
  final Future<bool> Function()? _onUnauthorized;

  /// Subscribe to [baseUrl] (e.g. `…/v1/recall/{thread}/stream`). The returned
  /// stream is single-subscription; cancelling it tears down the socket. A
  /// terminal `done`/`failed`/`completed` is forwarded but does NOT auto-close —
  /// the caller decides when to stop listening.
  Stream<SseEvent> stream(String baseUrl) {
    final controller = StreamController<SseEvent>();
    var lastSeq = 0;
    StreamSubscription<RawSseFrame>? sub;
    var closed = false;
    var triedRefresh = false; // AUTH-R4: one refresh attempt per stream

    void connect() {
      if (closed) return;
      final sep = baseUrl.contains('?') ? '&' : '?';
      // Replay strictly AFTER the last good seq (the ring re-sends ≤20 frames).
      final replay = lastSeq > 0 ? '&since_seq=$lastSeq' : '';
      final url = '$baseUrl${sep}token=${Uri.encodeQueryComponent(_token())}$replay';
      sub = _transport.connect(url).listen(
        (frame) {
          if (frame.id <= lastSeq) return; // dedup replayed frames (S4-R21)
          final event = _decode(frame);
          // Skip malformed + unknown frames WITHOUT advancing the replay cursor:
          // a future/unknown event must not poison `since_seq` (the plan pins
          // "replay from the last GOOD id"), so the ring may safely re-send it on
          // reconnect — it just gets skipped again, never duplicating into the UI.
          if (event == null) return; // malformed → skip, never crash (S4-R23)
          if (event is UnknownEvent) return; // forward-compat skip (S4-R23)
          lastSeq = frame.id; // only known, surfaced events advance the cursor
          if (!controller.isClosed) controller.add(event);
        },
        onError: (Object e) {
          if (e is SseDropped) {
            sub?.cancel();
            connect(); // reconnect with the replay offset (S4-R21)
          } else if (e is SseHttpError &&
              e.statusCode == 401 &&
              !triedRefresh &&
              _onUnauthorized != null) {
            // AUTH-R4: a setup-401 (expired/rotated ?token=) → refresh once, then
            // reconnect with the new token. A second 401 (below) is surfaced.
            triedRefresh = true;
            sub?.cancel();
            _onUnauthorized().then((ok) {
              if (closed) return;
              if (ok) {
                connect(); // reconnect — _token() now returns the fresh JWT
              } else if (!controller.isClosed) {
                controller.addError(e);
              }
            });
          } else if (!controller.isClosed) {
            controller.addError(e);
          }
        },
        onDone: () {
          // A clean server-side close (not a drop) ends the stream.
          if (!closed && !controller.isClosed) controller.close();
        },
      );
    }

    controller.onCancel = () {
      closed = true;
      return sub?.cancel();
    };
    connect();
    return controller.stream;
  }

  /// Decode one envelope frame to a sealed [SseEvent], or null if the `data:`
  /// payload is not valid JSON / not a JSON object (skip, never throw).
  static SseEvent? _decode(RawSseFrame frame) {
    try {
      final decoded = jsonDecode(frame.data);
      if (decoded is! Map) return null;
      final env = RuntimeEventEnvelope.fromJson(decoded.cast<String, dynamic>());
      // Map the envelope's forward-compat `event` String + `payload` onto the
      // sealed taxonomy (the same decoder the FFI egress uses). thread_id rides
      // along so `session_started` surfaces the conversation id for continue
      // (REC-R7) — the server's session_started payload carries only {mode}.
      return SseEvent.fromJson(
          {'event': env.event, 'payload': env.payload, 'thread_id': env.threadId});
    } catch (_) {
      return null;
    }
  }
}

/// The production [SseTransport]: opens a streamed GET to [url], expects a
/// `text/event-stream` body, and parses the line protocol (`id:`, `data:`,
/// blank-line frame boundary). Comment lines (`:keep-alive` heartbeats) and the
/// `event:` line are ignored. A premature body end surfaces as [SseDropped] so
/// the [SseClient] reconnects.
class HttpSseTransport implements SseTransport {
  HttpSseTransport({http.Client? client}) : _client = client ?? http.Client();
  final http.Client _client;

  @override
  Stream<RawSseFrame> connect(String url) {
    final controller = StreamController<RawSseFrame>();
    () async {
      http.StreamedResponse resp;
      try {
        final req = http.Request('GET', Uri.parse(url))
          ..headers['Accept'] = 'text/event-stream'
          ..headers['Cache-Control'] = 'no-cache';
        resp = await _client.send(req);
      } catch (e) {
        if (!controller.isClosed) controller.addError(const SseDropped());
        await controller.close();
        return;
      }
      if (resp.statusCode >= 400) {
        controller.addError(SseHttpError(resp.statusCode));
        await controller.close();
        return;
      }
      int? id;
      final dataLines = <String>[];
      void flush() {
        if (dataLines.isEmpty) return;
        final data = dataLines.join('\n');
        dataLines.clear();
        if (id != null && !controller.isClosed) {
          controller.add(RawSseFrame(id: id!, data: data));
        }
        id = null;
      }

      late StreamSubscription<String> lines;
      lines = resp.stream
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .listen(
        (line) {
          if (line.isEmpty) {
            flush(); // frame boundary
          } else if (line.startsWith(':')) {
            // comment / heartbeat — ignore
          } else if (line.startsWith('id:')) {
            id = int.tryParse(line.substring(3).trim());
          } else if (line.startsWith('data:')) {
            dataLines.add(line.substring(5).trimLeft());
          }
          // `event:` and any other field are intentionally ignored.
        },
        onError: (Object e) {
          if (!controller.isClosed) controller.addError(const SseDropped());
        },
        onDone: () {
          flush();
          // The body ended without a clean app-level close → treat as a drop so
          // the client reconnects with the replay offset.
          if (!controller.isClosed) controller.addError(const SseDropped());
        },
        cancelOnError: false,
      );
      controller.onCancel = () => lines.cancel();
    }();
    return controller.stream;
  }
}

/// A non-retryable SSE setup failure (e.g. 401 on a bad/expired `?token=`).
class SseHttpError implements Exception {
  const SseHttpError(this.statusCode);
  final int statusCode;
  @override
  String toString() => 'SseHttpError($statusCode)';
}
