// QCue S4-R20/R21/R23: the SSE client contract — `?token=` auth (pitfall #15),
// replay-on-reconnect via the last `id:`/seq (`?since_seq=`), unknown-event skip
// (forward-compat, never throw), and decoding each `data:` frame as a
// RuntimeEventEnvelope mapped onto the sealed recall/dream taxonomy.
//
// Drives a fake raw-frame transport so connect/drop/reconnect are deterministic
// (no sockets); the in-process HttpServer end-to-end path is covered in
// http_api_client_test.dart.
import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/net/sse_client.dart';

/// A fake raw-frame source: one [StreamController] per `connect()` so the test
/// can emit envelope frames, inject a drop, then assert the reconnect URL.
class FakeSse implements SseTransport {
  final connectedUrls = <String>[];
  final _ctrls = <StreamController<RawSseFrame>>[];

  @override
  Stream<RawSseFrame> connect(String url) {
    connectedUrls.add(url);
    final c = StreamController<RawSseFrame>();
    _ctrls.add(c);
    return c.stream;
  }

  void emit(int conn, RawSseFrame f) => _ctrls[conn].add(f);
  void drop(int conn) => _ctrls[conn].addError(const SseDropped());
  void fail401(int conn) => _ctrls[conn].addError(const SseHttpError(401));
  int get connections => _ctrls.length;
}

/// One recall-taxonomy envelope frame as the server actually serializes it:
/// `data:` is the whole RuntimeEventEnvelope, `id:` carries the seq.
RawSseFrame env(int seq, String event, Map<String, dynamic> payload) =>
    RawSseFrame(
      id: seq,
      data: '{"schema_version":1,"thread_id":"th-1","turn_id":null,'
          '"seq":$seq,"event":"$event","payload":${_json(payload)}}',
    );

String _json(Map<String, dynamic> m) {
  final parts = m.entries.map((e) {
    final v = e.value;
    final encoded = v is String ? '"$v"' : '$v';
    return '"${e.key}":$encoded';
  });
  return '{${parts.join(',')}}';
}

void main() {
  test('S4-R20: the connect URL carries the JWT as ?token= (never a header)',
      () {
    final t = FakeSse();
    SseClient(t, token: () => 'tok-1')
        .stream('https://app/v1/recall/th-1/stream')
        .listen((_) {});
    expect(t.connectedUrls.single, contains('token=tok-1'));
  });

  test('S4-R24: each data: envelope decodes to its sealed taxonomy variant', () {
    final t = FakeSse();
    final seen = <SseEvent>[];
    SseClient(t, token: () => 'tok')
        .stream('https://app/v1/recall/th-1/stream')
        .listen(seen.add);
    return Future<void>.delayed(Duration.zero, () {
      t.emit(0, env(1, 'session_started', {'mode': 'recall'}));
      t.emit(0, env(2, 'message_delta', {'delta': 'You decided '}));
      t.emit(0, env(3, 'tool_call', {'tool': 'recall_search'}));
      t.emit(0, env(4, 'tool_result', {'tool': 'recall_search', 'hits': 1}));
      t.emit(
        0,
        env(5, 'citation',
            {'rel_path': 'recall-architecture.md', 'start_line': 42, 'end_line': 47}),
      );
      t.emit(0, env(6, 'usage', {'input': 1280, 'output': 64, 'reasoning': 22}));
      t.emit(0, env(7, 'done', {'ok': true}));
    }).then((_) => Future<void>.delayed(Duration.zero)).then((_) {
      expect(seen.whereType<SessionStarted>(), isNotEmpty);
      expect(seen.whereType<MessageDelta>().map((e) => e.text), ['You decided ']);
      expect(seen.whereType<ToolCall>().single.name, 'recall_search');
      expect(seen.whereType<ToolResult>(), isNotEmpty);
      final cite = seen.whereType<CitationEvent>().single.citation;
      expect(cite.label, 'recall-architecture.md:42-47');
      final usage = seen.whereType<UsageEvent>().single;
      expect(usage.inputTokens, 1280);
      expect(usage.reasoningTokens, 22);
      expect(seen.whereType<DoneEvent>(), isNotEmpty);
    });
  });

  test(
      'S4-R21+R23: reconnect replays from the last seq (?since_seq=), no dup '
      'deltas, unknown event kind skipped', () async {
    final t = FakeSse();
    final seen = <SseEvent>[];
    SseClient(t, token: () => 'tok')
        .stream('https://app/v1/recall/th-1/stream')
        .listen(seen.add);
    await Future<void>.delayed(Duration.zero);

    t.emit(0, env(1, 'message_delta', {'delta': 'a'}));
    t.emit(0, env(2, 'message_delta', {'delta': 'b'}));
    t.emit(0, env(3, 'from_the_future', {})); // unknown → skipped, never thrown
    t.drop(0); // mid-answer drop
    await Future<void>.delayed(Duration.zero);

    // Reconnect URL requests replay strictly AFTER the last good seq (2).
    expect(t.connections, 2);
    expect(t.connectedUrls.last, contains('since_seq=2'));
    expect(t.connectedUrls.last, contains('token=tok'));

    // The replay ring re-sends seq 2 (already seen) + the new seq 3.
    t.emit(1, env(2, 'message_delta', {'delta': 'b'}));
    t.emit(1, env(3, 'message_delta', {'delta': 'c'}));
    await Future<void>.delayed(Duration.zero);

    final texts = seen.whereType<MessageDelta>().map((e) => e.text).toList();
    expect(texts, ['a', 'b', 'c']); // no duplicate 'b'; the unknown never appeared
    expect(seen.whereType<UnknownEvent>(), isEmpty); // forward-compat skip
  });

  test('S4-R23: a malformed data: frame is skipped, never crashes the stream',
      () async {
    final t = FakeSse();
    final seen = <SseEvent>[];
    final errors = <Object>[];
    SseClient(t, token: () => 'tok')
        .stream('https://app/v1/recall/th-1/stream')
        .listen(seen.add, onError: errors.add);
    await Future<void>.delayed(Duration.zero);

    t.emit(0, const RawSseFrame(id: 1, data: 'not json at all'));
    t.emit(0, env(2, 'message_delta', {'delta': 'ok'}));
    await Future<void>.delayed(Duration.zero);

    expect(errors, isEmpty);
    expect(seen.whereType<MessageDelta>().single.text, 'ok');
  });

  test('AUTH-R4: an SSE setup 401 triggers ONE refresh + reconnect with the new token',
      () async {
    final t = FakeSse();
    var token = 'stale';
    var refreshes = 0;
    final seen = <SseEvent>[];
    final errors = <Object>[];
    SseClient(
      t,
      token: () => token,
      onUnauthorized: () async {
        refreshes++;
        token = 'fresh'; // the refresh minted a new bearer
        return true;
      },
    ).stream('https://app/v1/recall/th-1/stream').listen(seen.add, onError: errors.add);
    await Future<void>.delayed(Duration.zero);

    // First connection is rejected with a setup-401 …
    t.fail401(0);
    await Future<void>.delayed(Duration.zero);

    // … the client refreshed exactly once and reconnected with the new token.
    expect(refreshes, 1);
    expect(t.connections, 2);
    expect(t.connectedUrls.last, contains('token=fresh'));
    expect(errors, isEmpty, reason: 'a recoverable 401 must not surface as a stream error');
  });

  test('AUTH-R4: a SECOND consecutive 401 surfaces as an error (no infinite loop)',
      () async {
    final t = FakeSse();
    var refreshes = 0;
    final errors = <Object>[];
    SseClient(
      t,
      token: () => 'tok',
      onUnauthorized: () async {
        refreshes++;
        return true; // refresh "succeeds" but the token is still rejected
      },
    ).stream('https://app/v1/recall/th-1/stream').listen((_) {}, onError: errors.add);
    await Future<void>.delayed(Duration.zero);

    t.fail401(0); // first 401 → refresh + reconnect
    await Future<void>.delayed(Duration.zero);
    t.fail401(1); // second 401 on the fresh connection → surface, do not loop
    await Future<void>.delayed(Duration.zero);

    expect(refreshes, 1, reason: 'only one refresh attempt per stream');
    expect(errors.whereType<SseHttpError>(), isNotEmpty);
  });

  test('REC-R7: a session_started frame surfaces the envelope thread_id (continue)',
      () async {
    final t = FakeSse();
    final seen = <SseEvent>[];
    SseClient(t, token: () => 'tok')
        .stream('https://app/v1/recall/th-1/stream')
        .listen(seen.add);
    await Future<void>.delayed(Duration.zero);
    // The server's session_started payload carries only {mode}; the thread id
    // lives on the ENVELOPE (thread_id). The decode must surface it so the
    // client can reuse the thread on the next ask (continue).
    t.emit(0, env(1, 'session_started', {'mode': 'recall'}));
    await Future<void>.delayed(Duration.zero);
    expect(seen.whereType<SessionStarted>().single.threadId, 'th-1',
        reason: 'the SSE decode must carry thread_id from the envelope (REC-R7)');
  });
}
