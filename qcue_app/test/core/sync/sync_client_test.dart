// QCue Sync Phase 1 (Task 10): the SyncClient registers the device, pulls the
// change feed, and applies the delta into the existing offline cache. A cold
// pull (snapshot) writes ideas + wiki into the cache (preserving unflushed
// queued captures, like putFeed); a warm pull applies incremental ops by a local
// reducer (idea.create appends a capture; wiki_page.set_body updates the wiki
// cache). The cursor is persisted in sync_meta and rides as `since` on the next
// pull.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';
import 'package:qcue_app/core/sync/sync_client.dart';
import 'package:qcue_app/core/sync/sync_dtos.dart';

/// A scripted api that hands back a fixed register response + a queue of deltas
/// (one per pull), recording the `since` each pull was issued with.
class _FakeSyncApi {
  _FakeSyncApi(this._deltas);
  final List<SyncDelta> _deltas;
  int _next = 0;
  final List<int> sinceSeen = [];
  final List<String> registeredPlatforms = [];

  Future<DeviceReg> registerDevice(String platform) async {
    registeredPlatforms.add(platform);
    return const DeviceReg(deviceId: 'dev-7', siteId: 3);
  }

  Future<SyncDelta> pullSync({required int since}) async {
    sinceSeen.add(since);
    return _deltas[_next++];
  }
}

void main() {
  SyncSnapshot snapshot() => const SyncSnapshot(
        ideas: [
          IdeaSnap(
              id: 'idea-a',
              body: 'first',
              origin: 'capture',
              capturedAt: '2026-06-15T00:00:00Z'),
          IdeaSnap(
              id: 'idea-b',
              body: 'second',
              origin: 'voice',
              capturedAt: '2026-06-15T00:01:00Z'),
        ],
        wikiPages: [
          WikiPageSnap(
              slug: 'note-a',
              title: 'Note A',
              contentHash: 'h1',
              syncVersion: 1),
        ],
      );

  test('register() stores device_id + site_id in sync_meta', () async {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 100);
    final api = _FakeSyncApi([const SyncDelta(cursor: 0)]);
    final sync = SyncClient(
      registerDevice: api.registerDevice,
      pullSync: api.pullSync,
      cache: cache,
      platform: 'android',
    );

    await sync.register();
    expect(api.registeredPlatforms, ['android']);
    final meta = cache.syncMeta();
    expect(meta!.deviceId, 'dev-7');
    expect(meta.siteId, 3);
  });

  test('pull() applies a snapshot into the cache + persists the cursor',
      () async {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 100);
    final api = _FakeSyncApi([SyncDelta(cursor: 12, snapshot: snapshot())]);
    final sync = SyncClient(
      registerDevice: api.registerDevice,
      pullSync: api.pullSync,
      cache: cache,
      platform: 'android',
    );

    await sync.pull();

    // 2 ideas in the feed; 1 wiki page in the wiki cache.
    expect(cache.feed().map((i) => i.id).toSet(), {'idea-a', 'idea-b'});
    expect(cache.wikiIndex().map((p) => p.slug), ['note-a']);
    // The first pull is cold (since:0) and persists the snapshot cursor.
    expect(api.sinceSeen, [0]);
    expect(cache.syncMeta()!.cursor, 12);
  });

  test('a snapshot pull preserves an unflushed queued capture (like putFeed)',
      () async {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 100);
    final queued = cache.enqueueCapture(body: 'mine', origin: 'capture');
    final api = _FakeSyncApi([SyncDelta(cursor: 5, snapshot: snapshot())]);
    final sync = SyncClient(
      registerDevice: api.registerDevice,
      pullSync: api.pullSync,
      cache: cache,
      platform: 'android',
    );

    await sync.pull();

    // The queued capture is NOT evicted by the snapshot merge.
    expect(cache.feed().map((i) => i.id), contains(queued.id));
    expect(cache.outbound().map((q) => q.idea.id), contains(queued.id));
    // The snapshot rows are present too.
    expect(cache.feed().map((i) => i.id), containsAll(['idea-a', 'idea-b']));
  });

  test('pull() reports whether a delta was applied (drives the UI refresh)',
      () async {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 100);
    final api = _FakeSyncApi([
      SyncDelta(cursor: 12, snapshot: snapshot()), // a snapshot changed the cache
      const SyncDelta(cursor: 12), // an empty warm delta changed nothing
    ]);
    final sync = SyncClient(
      registerDevice: api.registerDevice,
      pullSync: api.pullSync,
      cache: cache,
      platform: 'android',
    );

    expect(await sync.pull(), isTrue, reason: 'snapshot applied → caller must refresh the UI');
    expect(await sync.pull(), isFalse, reason: 'empty delta → no refresh, no wasted re-fetch');
  });

  test('a second pull sends since=<cursor> and applies incremental ops',
      () async {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 100);
    final api = _FakeSyncApi([
      SyncDelta(cursor: 12, snapshot: snapshot()),
      // incremental: a new idea.create + a wiki_page.set_body update.
      const SyncDelta(cursor: 20, ops: [
        SyncOp(
          hlcWallMs: 100,
          hlcLamport: 1,
          siteId: 5,
          entityKind: 'idea',
          entityRef: 'idea-c',
          op: {
            'create': {
              'body': 'third',
              'origin': 'capture',
              'captured_at': '2026-06-15T00:02:00Z',
            }
          },
        ),
        SyncOp(
          hlcWallMs: 101,
          hlcLamport: 2,
          siteId: 5,
          entityKind: 'wiki_page',
          entityRef: 'note-a',
          op: {'set_body': '# Note A\n\nupdated body'},
        ),
      ]),
    ]);
    final sync = SyncClient(
      registerDevice: api.registerDevice,
      pullSync: api.pullSync,
      cache: cache,
      platform: 'android',
    );

    await sync.pull(); // snapshot (since:0) → cursor 12
    await sync.pull(); // incremental (since:12) → cursor 20

    expect(api.sinceSeen, [0, 12]);
    // idea.create appended a capture.
    expect(cache.feed().map((i) => i.id), contains('idea-c'));
    // wiki_page.set_body updated the cached page body.
    final page = cache.wikiPage('note-a');
    expect(page, isNotNull);
    expect(page!.bodyMarkdown, contains('updated body'));
    // The cursor advanced + persisted.
    expect(cache.syncMeta()!.cursor, 20);
  });
}
