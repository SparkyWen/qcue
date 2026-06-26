import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/recall_conversation.dart';

void main() {
  test('ConversationSummary.fromJson maps the wire shape', () {
    final s = ConversationSummary.fromJson(const {
      'id': 'th-1',
      'title': 'Postgres migration',
      'updated_at': '2026-06-16T00:00:00Z',
      'last_snippet': '…partial indexes',
    });
    expect(s.id, 'th-1');
    expect(s.title, 'Postgres migration');
    expect(s.lastSnippet, '…partial indexes');
  });

  test('ConversationMessage.fromJson maps role + content', () {
    final m = ConversationMessage.fromJson(const {'role': 'assistant', 'content': 'hi'});
    expect(m.role, 'assistant');
    expect(m.content, 'hi');
  });
}
