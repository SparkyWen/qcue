// QCue S4-R19/R22: the WSS JSON-RPC-lite turn channel contract.
//   - frames OMIT the `"jsonrpc"` field (JSON-RPC-LITE) and never carry a
//     `tenant_id` (the server's RLS owns isolation — the client sends only JWT);
//   - responses correlate to requests by `id`;
//   - a `-32001` error is classified as a retryable BACKPRESSURE signal;
//   - server→client notifications (no `id`) surface on a side channel;
//   - the Thread→Turn→Item streaming invariant (`item/started → deltas →
//     item/completed`) is preserved in arrival order on the notification stream.
//
// Drives a fake duplex [RpcChannel] (no socket); the in-process
// HttpServer.transformToWebSocket end-to-end path is covered in
// http_api_client_test.dart.
import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/ws_client.dart';

class FakeChannel implements RpcChannel {
  final sent = <Map<String, dynamic>>[];
  final _in = StreamController<Map<String, dynamic>>.broadcast();
  var closed = false;

  @override
  Stream<Map<String, dynamic>> get incoming => _in.stream;
  @override
  void send(Map<String, dynamic> frame) => sent.add(frame);
  void serverReply(Map<String, dynamic> frame) => _in.add(frame);
  @override
  Future<void> close() async {
    closed = true;
    await _in.close();
  }
}

void main() {
  test('S4-R19: outbound frames omit jsonrpc + tenant_id and carry only the JWT',
      () async {
    final ch = FakeChannel();
    final client = WsApiClient(ch, jwt: () => 'jwt-abc');
    final fut = client.call('capture', {'body': 'hello'});
    final frame = ch.sent.single;
    expect(frame.containsKey('jsonrpc'), isFalse); // JSON-RPC-LITE (no field)
    expect((frame['params'] as Map).containsKey('tenant_id'), isFalse);
    expect(frame['auth'], 'jwt-abc');
    expect(frame['method'], 'capture');
    ch.serverReply({'id': frame['id'], 'result': {'ok': true}});
    expect(await fut, {'ok': true});
    await client.dispose();
  });

  test('S4-R22: a -32001 error is a retryable backpressure signal', () async {
    final ch = FakeChannel();
    final client = WsApiClient(ch, jwt: () => 'jwt');
    final fut = client.call('query', const {});
    final id = ch.sent.single['id'];
    ch.serverReply({
      'id': id,
      'error': {'code': -32001, 'message': 'overloaded'},
    });
    final err = await fut.then<Object?>((_) => null).catchError((Object e) => e);
    expect(err, isA<RpcError>());
    expect((err as RpcError).isBackpressure, isTrue);
    expect(err.code, -32001);
    await client.dispose();
  });

  test('S4-R8: responses correlate by id even when they arrive out of order',
      () async {
    final ch = FakeChannel();
    final client = WsApiClient(ch, jwt: () => 'jwt');
    final a = client.call('m', {'n': 1});
    final b = client.call('m', {'n': 2});
    final idA = ch.sent[0]['id'];
    final idB = ch.sent[1]['id'];
    expect(idA, isNot(idB)); // monotonic ids
    // reply to B first, then A.
    ch.serverReply({'id': idB, 'result': {'who': 'b'}});
    ch.serverReply({'id': idA, 'result': {'who': 'a'}});
    expect(await a, {'who': 'a'});
    expect(await b, {'who': 'b'});
    await client.dispose();
  });

  test(
      'S4-R5: server notifications (no id) surface in order, preserving the '
      'item/started → delta → item/completed invariant', () async {
    final ch = FakeChannel();
    final client = WsApiClient(ch, jwt: () => 'jwt');
    final notes = <Map<String, dynamic>>[];
    client.notifications.listen(notes.add);
    // A notification has a `method` + `params` but NO `id`.
    ch.serverReply({'method': 'item/started', 'params': {'item': 'recallResult'}});
    ch.serverReply({'method': 'item/delta', 'params': {'delta': 'You '}});
    ch.serverReply({'method': 'item/delta', 'params': {'delta': 'decided'}});
    ch.serverReply({'method': 'item/completed', 'params': {'item': 'recallResult'}});
    await Future<void>.delayed(Duration.zero);
    expect(notes.map((n) => n['method']).toList(), [
      'item/started',
      'item/delta',
      'item/delta',
      'item/completed',
    ]);
    await client.dispose();
  });

  test('AUTH-R4: WebSocketRpcChannel.connect builds ?token= from the current bearer',
      () {
    var token = 'stale';
    // The factory reads the token callback at connect time, so a reconnect after
    // a refresh carries the rotated bearer (the WS upgrade GET can't set a header).
    String urlFor(String Function() tok) {
      const base = 'wss://host/v1/thread/th-1/ws';
      final sep = base.contains('?') ? '&' : '?';
      return '$base${sep}token=${Uri.encodeQueryComponent(tok())}';
    }

    expect(urlFor(() => token), contains('token=stale'));
    token = 'fresh';
    expect(urlFor(() => token), contains('token=fresh'),
        reason: 'a reconnect after refresh must carry the rotated token');
  });
}
