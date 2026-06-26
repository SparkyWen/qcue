// QCue Sync Phase 1 (Task 11): the api seam gains registerDevice + pullSync so
// the keyless demo + tests have sync without a backend. The seeded StubApiClient
// builds a deterministic snapshot from its seeded ideas + wiki pages, so a first
// pull (since:0) returns those rows; registerDevice returns a stub device.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';

void main() {
  test('Task 11: StubApiClient.seeded().pullSync(since:0) returns a snapshot '
      'built from the seeds', () async {
    final api = StubApiClient.seeded();
    final delta = await api.pullSync(since: 0);

    // A cold pull is a snapshot bootstrap, not incremental ops.
    expect(delta.snapshot, isNotNull);
    expect(delta.ops, isEmpty);
    final snap = delta.snapshot!;

    // The 6 seeded ideas + 6 seeded wiki pages surface in the snapshot.
    expect(snap.ideas, isNotEmpty);
    expect(snap.wikiPages, isNotEmpty);
    expect(snap.ideas.map((i) => i.id), contains('i-1'));
    expect(snap.wikiPages.map((p) => p.slug), contains('auto-dream'));

    // Wiki snap rows carry the listing fields (slug/title/hash/version), no body.
    final page = snap.wikiPages.firstWhere((p) => p.slug == 'auto-dream');
    expect(page.title, 'Auto-Dream');
    expect(page.contentHash, isNotEmpty);

    // The cursor is non-negative (monotone seq cursor).
    expect(delta.cursor, greaterThanOrEqualTo(0));
  });

  test('Task 11: a warm pull (since:cursor) is incremental (no snapshot)',
      () async {
    final api = StubApiClient.seeded();
    final cold = await api.pullSync(since: 0);
    final warm = await api.pullSync(since: cold.cursor);
    // The seeded stub has no new ops past the snapshot watermark.
    expect(warm.snapshot, isNull);
    expect(warm.ops, isEmpty);
    expect(warm.cursor, cold.cursor);
  });

  test('Task 11: registerDevice returns a stub device + site_id', () async {
    final api = StubApiClient.seeded();
    final reg = await api.registerDevice('android');
    expect(reg.deviceId, isNotEmpty);
    expect(reg.siteId, greaterThanOrEqualTo(1)); // device site_ids start at 1
  });
}
