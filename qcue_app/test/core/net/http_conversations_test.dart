import 'dart:convert';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:qcue_app/core/net/http_api_client.dart';
import 'package:qcue_app/core/net/qcue_config.dart';

void main() {
  test('listConversations + getConversationMessages parse the wire shape', () async {
    final mock = MockClient((req) async {
      if (req.url.path == '/v1/conversations') {
        return http.Response(jsonEncode({
          'conversations': [
            {'id': 'th-1', 'title': 'T', 'updated_at': '2026-06-16T00:00:00Z', 'last_snippet': 's'}
          ]
        }), 200, headers: {'content-type': 'application/json'});
      }
      if (req.url.path == '/v1/conversations/th-1/messages') {
        return http.Response(jsonEncode({
          'messages': [
            {'role': 'user', 'content': 'hi'},
            {'role': 'assistant', 'content': 'yo'}
          ]
        }), 200, headers: {'content-type': 'application/json'});
      }
      return http.Response('{}', 404);
    });
    final api = HttpApiClient(QcueConfig(baseUrl: 'http://x'),
        tokens: InMemoryTokenStore(access: 'tok'), httpClient: mock);
    final convos = await api.listConversations();
    expect(convos.single.id, 'th-1');
    final msgs = await api.getConversationMessages('th-1');
    expect(msgs.map((m) => m.role), ['user', 'assistant']);
  });
}
