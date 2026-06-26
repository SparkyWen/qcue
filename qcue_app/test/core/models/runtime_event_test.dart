import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/runtime_event.dart';
import 'package:qcue_app/core/models/sse_event.dart' show Citation;

void main() {
  group('RuntimeEventEnvelope (S4-R5)', () {
    test('decodes the exact wire shape the Rust side emits', () {
      // Mirrors protocol::RuntimeEventEnvelope serde output (snake_case fields).
      final j = {
        'schema_version': 1,
        'thread_id': '11111111-1111-1111-1111-111111111111',
        'turn_id': '22222222-2222-2222-2222-222222222222',
        'seq': 7,
        'event': 'itemDelta',
        'payload': {'type': 'agentMessage', 'delta': 'hi'},
      };
      final env = RuntimeEventEnvelope.fromJson(j);
      expect(env.schemaVersion, 1);
      expect(env.threadId, '11111111-1111-1111-1111-111111111111');
      expect(env.turnId, '22222222-2222-2222-2222-222222222222');
      expect(env.seq, 7);
      expect(env.event, 'itemDelta');
      expect(env.knownKind, RuntimeEventKind.itemDelta);
      expect(env.payload['delta'], 'hi');
    });

    test('forward-compat: an UNKNOWN event string decodes, not throws', () {
      final env = RuntimeEventEnvelope.fromJson({
        'schema_version': 99,
        'event': 'someFutureKind',
        'payload': {'anything': true},
      });
      expect(env.event, 'someFutureKind');
      expect(env.knownKind, isNull); // unknown, but survived
      expect(env.payload['anything'], true);
    });

    test('forward-compat: a minimal envelope (defaults) decodes', () {
      final env = RuntimeEventEnvelope.fromJson({'event': 'threadStarted'});
      expect(env.schemaVersion, 0);
      expect(env.threadId, '');
      expect(env.turnId, isNull);
      expect(env.seq, 0);
      expect(env.payload, isEmpty);
    });

    test('the 8 known event kinds map to their camelCase wire tokens', () {
      expect(
        RuntimeEventKind.values.map(runtimeEventKindToWire).toSet(),
        {
          'threadStarted',
          'turnStarted',
          'itemStarted',
          'itemDelta',
          'itemCompleted',
          'turnCompleted',
          'usage',
          'error',
        },
      );
    });
  });

  group('Item taxonomy (S4-R5, camelCase tags)', () {
    final items = <Item>[
      const ItemIdeaCaptured(ideaId: 'i1', body: 'b'),
      const ItemVoiceTranscript(ideaId: 'i2', text: 't', provider: 'whisper'),
      const ItemWikiEdit(pageId: 'p1', slug: 's', op: WikiEditMerge('into')),
      const ItemRecallResult(
        answerDelta: 'a',
        citations: [Citation(relPath: 'x.md', startLine: 1, endLine: 2)],
      ),
      const ItemAgentMessage(delta: 'd'),
      const ItemDreamTurn(
        phase: DreamPhase.consolidate,
        pagesTouched: ['a', 'b'],
      ),
      const ItemReasoning(delta: 'r'),
      const ItemError(code: -1, message: 'boom'),
    ];

    test('every variant round-trips with value equality', () {
      for (final it in items) {
        expect(Item.fromJson(it.toJson()), it, reason: '${it.runtimeType}');
      }
    });

    test('variant tags are camelCase on the wire', () {
      expect(
        items.map((i) => i.toJson()['type']).toSet(),
        {
          'ideaCaptured',
          'voiceTranscript',
          'wikiEdit',
          'recallResult',
          'agentMessage',
          'dreamTurn',
          'reasoning',
          'error',
        },
      );
    });

    test('WikiEditOp variants round-trip (tag="type", Merge carries slug)', () {
      final ops = <WikiEditOp>[
        const WikiEditCreate(),
        const WikiEditUpdate(),
        const WikiEditMerge('target'),
        const WikiEditDelete(),
      ];
      for (final op in ops) {
        expect(WikiEditOp.fromJson(op.toJson()), op);
      }
      expect(const WikiEditMerge('t').toJson()['into_slug'], 't');
    });

    test('DreamPhase maps to its PascalCase wire token', () {
      expect(DreamPhase.values.map(dreamPhaseToJson).toSet(), {
        'Orient',
        'Gather',
        'Consolidate',
        'Prune',
      });
    });
  });
}
