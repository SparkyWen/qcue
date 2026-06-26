// QCue — the Axum backend emits `Content-Type: application/json` WITHOUT a charset. The Dart `http`
// package then decodes `Response.body` as LATIN-1, mangling multi-byte UTF-8 (Chinese) into mojibake
// (乱码). The client must decode response BYTES as UTF-8 explicitly. These reproduce the charset-less
// server response exactly and assert the decoded text round-trips intact.
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:qcue_app/core/net/http_api_client.dart';
import 'package:qcue_app/core/net/qcue_config.dart';

HttpApiClient _client(MockClient mock) => HttpApiClient(
      QcueConfig(baseUrl: 'http://test.local'),
      tokens: InMemoryTokenStore(access: 'jwt'),
      httpClient: mock,
    );

void main() {
  test('captures() decodes a charset-less application/json Chinese body as UTF-8', () async {
    const chinese = '今天的想法：复刻 Hermes harness 跑通 DeepSeek';
    final mock = MockClient((req) async => http.Response.bytes(
          utf8.encode(jsonEncode({
            'captures': [
              {
                'id': 'i-1',
                'kind': 'text',
                'body': chinese,
                'ingest_state': 'ingested',
                'captured_at': '2026-06-16T07:00:00Z',
              }
            ]
          })),
          200,
          // Axum exactly: application/json with NO "; charset=utf-8".
          headers: {'content-type': 'application/json'},
        ));
    final client = _client(mock);
    final feed = await client.captures();
    expect(feed.single.body, chinese, reason: 'UTF-8 must not be decoded as latin-1');
    await client.dispose();
  });

  test('a charset-less error body surfaces a UTF-8 message (not mojibake)', () async {
    const msg = '该邮箱已被注册';
    final mock = MockClient((req) async => http.Response.bytes(
          utf8.encode(jsonEncode({
            'error': {'code': -32603, 'message': msg}
          })),
          400,
          headers: {'content-type': 'application/json'},
        ));
    final client = _client(mock);
    await expectLater(
      () => client.captures(),
      throwsA(predicate((e) => e.toString().contains(msg))),
    );
    await client.dispose();
  });
}
