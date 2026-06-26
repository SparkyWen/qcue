import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';

void main() {
  test('seeded stub lists conversations and returns thread messages', () async {
    final api = StubApiClient.seeded();
    final convos = await api.listConversations();
    expect(convos, isNotEmpty);
    final msgs = await api.getConversationMessages(convos.first.id);
    expect(msgs, isNotEmpty);
    expect(msgs.first.role, anyOf('user', 'assistant'));
  });

  test('recallStream reuses a supplied threadId via SessionStarted', () async {
    final api = StubApiClient.seeded();
    final events = await api.recallStream('q', threadId: 'th-reused').toList();
    final started = events.whereType<SessionStarted>().first;
    expect(started.threadId, 'th-reused');
  });
}
