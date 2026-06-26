// QCue S4-R19/R22: the WSS JSON-RPC-lite turn channel (master §8). This is the
// bidirectional interactive surface — `Thread → Turn → Item` with the streaming
// invariant `item/started → deltas → item/completed`. Canonical rules:
//   - frames OMIT the `"jsonrpc"` field (JSON-RPC-LITE);
//   - the client sends ONLY the session JWT (in `auth`), NEVER a `tenant_id` —
//     the server's RLS owns tenant isolation (pitfall #14);
//   - responses correlate to requests by a monotonic integer `id`;
//   - a `-32001` error is a retryable server-overload BACKPRESSURE signal;
//   - server→client notifications (frames with a `method` but no `id`) surface
//     on a side channel in arrival order.
//
// The duplex byte transport is abstracted behind [RpcChannel] so the
// request/response correlation + backpressure classification are unit-testable
// without a socket; [WebSocketRpcChannel] is the production
// `web_socket_channel` transport (connects with the JWT as `?token=`, since the
// WS upgrade handshake — like SSE — can't reliably carry an `Authorization`
// header from every client).
import 'dart:async';
import 'dart:convert';

import 'package:web_socket_channel/web_socket_channel.dart';

/// A typed JSON-RPC-lite error (`{code, message}`). `-32001` is the server's
/// overload / cost-cap signal — see [isBackpressure].
class RpcError implements Exception {
  RpcError(this.code, this.message);
  final int code;
  final String message;

  /// The server is overloaded (or the daily cost cap was hit); the caller should
  /// retry with a capped backoff+jitter rather than surface a hard failure.
  bool get isBackpressure => code == -32001;

  /// The session JWT was rejected (`-32002`) — re-auth, do not retry.
  bool get isUnauthorized => code == -32002;

  @override
  String toString() => 'RpcError($code, $message)';
}

/// Abstracts the duplex transport so the client logic is testable with a fake.
abstract interface class RpcChannel {
  /// Decoded inbound frames (one JSON object per message).
  Stream<Map<String, dynamic>> get incoming;

  /// Send one outbound frame (the client never blocks on the socket).
  void send(Map<String, dynamic> frame);

  Future<void> close();
}

/// JSON-RPC-lite client over a duplex [RpcChannel]. Sends only the JWT; never a
/// tenant filter. Correlates replies by `id`; classifies `-32001` as
/// backpressure; fans out un-correlated notifications on [notifications].
class WsApiClient {
  // `jwt` is read on every call so a rotated token is picked up without
  // re-creating the client.
  // ignore: prefer_initializing_formals
  WsApiClient(this._channel, {required String Function() jwt}) : _jwt = jwt {
    _sub = _channel.incoming.listen(_onFrame, onError: _onError);
  }

  final RpcChannel _channel;
  final String Function() _jwt;
  late final StreamSubscription<Map<String, dynamic>> _sub;
  final _notifications = StreamController<Map<String, dynamic>>.broadcast();
  final _pending = <int, Completer<Map<String, dynamic>>>{};
  int _nextId = 1;

  /// Server→client notifications (frames with a `method` but no `id`): the
  /// `item/started → item/delta* → item/completed` turn stream, `usage`, etc.
  Stream<Map<String, dynamic>> get notifications => _notifications.stream;

  /// Issue one request and await its `result` payload. Throws [RpcError] on a
  /// JSON-RPC-lite error frame.
  Future<Map<String, dynamic>> call(
    String method, [
    Map<String, dynamic> params = const {},
  ]) {
    assert(
      !params.containsKey('tenant_id'),
      'never send tenant_id — the server RLS owns isolation (pitfall #14)',
    );
    final id = _nextId++;
    final completer = Completer<Map<String, dynamic>>();
    _pending[id] = completer;
    _channel.send({
      'id': id,
      'method': method,
      'params': params,
      'auth': _jwt(), // JWT only — no jsonrpc field, no tenant_id
    });
    return completer.future;
  }

  void _onFrame(Map<String, dynamic> frame) {
    final id = frame['id'];
    if (id is! int) {
      // No correlation id → a server notification (turn/item stream, usage…).
      if (frame['method'] != null && !_notifications.isClosed) {
        _notifications.add(frame);
      }
      return;
    }
    final completer = _pending.remove(id);
    if (completer == null || completer.isCompleted) return;
    final error = frame['error'];
    if (error is Map) {
      final e = error.cast<String, dynamic>();
      completer.completeError(RpcError(
        (e['code'] as num?)?.toInt() ?? -32603,
        e['message'] as String? ?? '',
      ));
    } else {
      completer
          .complete((frame['result'] as Map?)?.cast<String, dynamic>() ?? const {});
    }
  }

  void _onError(Object e) {
    // A transport-level failure fails every in-flight request (the caller may
    // retry once the channel reconnects).
    for (final c in _pending.values) {
      if (!c.isCompleted) c.completeError(e);
    }
    _pending.clear();
  }

  Future<void> dispose() async {
    await _sub.cancel();
    for (final c in _pending.values) {
      if (!c.isCompleted) c.completeError(StateError('client disposed'));
    }
    _pending.clear();
    await _notifications.close();
    await _channel.close();
  }
}

/// Production [RpcChannel] over `web_socket_channel`. Connects to [url] with the
/// JWT appended as `?token=` (the WS handshake can't reliably carry an
/// `Authorization` header), JSON-encodes outbound frames, and JSON-decodes the
/// inbound text stream. A socket drop closes [incoming]; the owning client
/// reconnects with the replay ring offset (the WSS turn channel resumes from the
/// last delivered `seq`, mirroring the SSE replay path).
class WebSocketRpcChannel implements RpcChannel {
  WebSocketRpcChannel._(this._ch);

  /// Open a socket to [url] (e.g. `wss://host/v1/thread/{thread}/ws`) with
  /// `?token=<jwt>`. The token is read via [token] at connect time so a reconnect
  /// after a refresh carries the rotated bearer (AUTH-R4 — the WS upgrade GET,
  /// like SSE, can't reliably carry an `Authorization` header).
  factory WebSocketRpcChannel.connect(String url, {required String Function() token}) {
    final sep = url.contains('?') ? '&' : '?';
    final full = '$url${sep}token=${Uri.encodeQueryComponent(token())}';
    return WebSocketRpcChannel._(WebSocketChannel.connect(Uri.parse(full)));
  }

  final WebSocketChannel _ch;

  @override
  Stream<Map<String, dynamic>> get incoming => _ch.stream.map((dynamic msg) {
        final decoded = jsonDecode(msg is String ? msg : utf8.decode(msg as List<int>));
        return (decoded as Map).cast<String, dynamic>();
      });

  @override
  void send(Map<String, dynamic> frame) => _ch.sink.add(jsonEncode(frame));

  @override
  Future<void> close() => _ch.sink.close();
}
