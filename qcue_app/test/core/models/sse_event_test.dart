import 'dart:convert';
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/sse_event.dart';

void main() {
  test('S4-R24: every recall+dream event maps to its sealed variant', () {
    final log = (jsonDecode(
      File('test/fixtures/recall_event_log.json').readAsStringSync(),
    ) as List)
        .cast<Map<String, dynamic>>();
    final parsed = log.map(SseEvent.fromJson).toList();
    expect(parsed.whereType<SessionStarted>(), isNotEmpty);
    expect(parsed.whereType<MessageDelta>(), isNotEmpty);
    expect(parsed.whereType<ToolCall>(), isNotEmpty);
    expect(parsed.whereType<ToolResult>(), isNotEmpty);
    expect(parsed.whereType<CitationEvent>(), isNotEmpty);
    expect(parsed.whereType<UsageEvent>(), isNotEmpty);
    expect(parsed.whereType<DoneEvent>(), isNotEmpty);
    expect(parsed.whereType<DreamStarted>(), isNotEmpty);
    expect(parsed.whereType<DreamProgress>(), isNotEmpty);
    expect(parsed.whereType<DreamCompleted>(), isNotEmpty);
  });

  test('S4-R23: unknown event discriminant becomes UnknownEvent (skipped)', () {
    final e = SseEvent.fromJson({'event': 'from_the_future', 'payload': {}});
    expect(e, isA<UnknownEvent>());
  });

  test('S4-R24: a message_delta carries its text', () {
    final e = SseEvent.fromJson({
      'event': 'message_delta',
      'payload': {'text': 'hi'},
    });
    expect((e as MessageDelta).text, 'hi');
  });

  test('S4-R24: a citation event carries the rel_path:line label', () {
    final e = SseEvent.fromJson({
      'event': 'citation',
      'payload': {'rel_path': 'source.md', 'start_line': 42, 'end_line': 42},
    });
    expect((e as CitationEvent).citation.label, 'source.md:42');
  });

  test('S4-R11: FfiError has the 5 typed variants', () {
    expect(FfiError.values.map((e) => e.name).toSet(), {
      'network',
      'noKey',
      'costCapped',
      'cancelled',
      'internal',
    });
  });
}
