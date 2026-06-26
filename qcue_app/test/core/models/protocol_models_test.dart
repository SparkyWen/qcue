import 'dart:convert';
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';

void main() {
  test('S4-R5: Idea round-trips for every ingest-state variant', () {
    final fixtures = (jsonDecode(
      File('test/fixtures/idea_all_variants.json').readAsStringSync(),
    ) as List)
        .cast<Map<String, dynamic>>();
    // Every ingest_state enum value is represented.
    expect(
      fixtures.map((m) => m['ingest_state']).toSet(),
      {'pending', 'ingesting', 'ingested', 'skipped_redundant', 'failed'},
    );
    for (final m in fixtures) {
      final idea = Idea.fromJson(m);
      expect(Idea.fromJson(idea.toJson()), idea); // value equality
    }
  });

  test('S4-R5: enums reject unknown values loudly (forward-compat surfaced)', () {
    expect(() => ingestStateFromJson('bogus'), throwsArgumentError);
  });

  test('S4-R5: WikiPage type covers the full taxonomy', () {
    for (final t in WikiPageType.values) {
      final s = wikiPageTypeToJson(t);
      expect(wikiPageTypeFromJson(s), t);
    }
    expect(WikiPageType.values.map(wikiPageTypeToJson).toSet(), {
      'entity',
      'concept',
      'source',
      'index',
      'log',
      'contradiction',
      'schema',
      'comparison',
      'overview',
    });
  });

  test('S4-R5: every wire enum maps to its exact snake_case token', () {
    expect(IngestState.values.map(ingestStateToJson).toSet(), {
      'pending',
      'ingesting',
      'ingested',
      'skipped_redundant',
      'failed',
    });
    expect(JobState.values.map(jobStateToJson).toSet(), {
      'pending',
      'leased',
      'done',
      'failed',
      'skipped',
      'canceled',
    });
    expect(JobKind.values.map(jobKindToJson).toSet(), {
      'ingest',
      'lint',
      'dream',
      'transcribe',
      'sync_materialize',
      'export',
    });
    expect(ApprovalStatus.values.map(approvalStatusToJson).toSet(), {
      'pending',
      'approved',
      'rejected',
      'expired',
    });
    expect(CredStatus.values.map(credStatusToJson).toSet(), {
      'ok',
      'exhausted',
      'dead',
    });
  });
}
