// QCue S4-R25/R28: the production [SqliteCacheStore] (sqlite3 via dart:ffi) is
// the persistent backing for the offline cache. On this Linux host the native
// lib is `libsqlite3.so.0` (no unversioned symlink), so the store opens the
// versioned name. This test exercises the REAL persistent impl end-to-end:
// feed + queue + wiki rows survive a reopen of the same on-disk database.
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';
import 'package:qcue_app/core/offline/sqlite_cache_store.dart';

Idea _idea(String id) => Idea(
      id: id,
      tenantId: 't',
      userId: 'u',
      kind: IdeaKind.text,
      body: 'body-$id',
      origin: 'capture',
      ingestState: IngestState.pending,
      capturedAt: DateTime.parse('2026-06-13T00:00:00Z'),
    );

WikiPage _page(String slug) => WikiPage(
      id: 'w-$slug',
      type: WikiPageType.concept,
      slug: slug,
      title: slug,
      summary: 's',
      bodyMarkdown: '# $slug',
      updated: DateTime.parse('2026-06-13T00:00:00Z'),
    );

void main() {
  late Directory dir;
  late String dbPath;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('qcue_cache_test');
    dbPath = '${dir.path}/cache.db';
  });
  tearDown(() {
    if (dir.existsSync()) dir.deleteSync(recursive: true);
  });

  test('persists feed + queue across a reopen (durability)', () {
    final store = SqliteCacheStore.open(dbPath);
    store.writeFeed([_idea('a'), _idea('b')]);
    store.writeQueue(
        [OutboundCapture(idea: _idea('a'), idempotencyKey: 'key-a')]);
    store.dispose();

    final reopened = SqliteCacheStore.open(dbPath);
    expect(reopened.readFeed().map((i) => i.id), ['a', 'b']);
    final q = reopened.readQueue();
    expect(q.single.idea.id, 'a');
    expect(q.single.idempotencyKey, 'key-a');
    reopened.dispose();
  });

  test('the wiki read-cache survives a reopen', () {
    final store = SqliteCacheStore.open(dbPath);
    store.writeWiki([_page('alpha'), _page('beta')]);
    store.dispose();

    final reopened = SqliteCacheStore.open(dbPath);
    final pages = reopened.readWiki();
    expect(pages.map((p) => p.slug).toSet(), {'alpha', 'beta'});
    expect(pages.firstWhere((p) => p.slug == 'alpha').bodyMarkdown, '# alpha');
    reopened.dispose();
  });

  test('IdeaCache over the sqlite store enqueues + flushes durably', () async {
    final store = SqliteCacheStore.open(dbPath);
    final cache = IdeaCache(store, feedCap: 100);
    final q = cache.enqueueCapture(body: 'durable', origin: 'capture');
    expect(cache.outbound().single.idea.id, q.id);

    final seen = <String>[];
    await cache.flush((c) async => seen.add(c.idempotencyKey));
    expect(seen, hasLength(1));
    expect(cache.outbound(), isEmpty);
    expect(cache.feed().single.ingestState, IngestState.ingested);
    // Close the DB handle before teardown deletes the temp dir — Windows holds a
    // lock on the open file (POSIX would allow the unlink), matching the other
    // cases above which dispose their stores.
    store.dispose();
  });

  test('clear() wipes feed + queue + wiki + sync meta, durably (sqlite)', () {
    final store = SqliteCacheStore.open(dbPath);
    store.writeFeed([_idea('a'), _idea('b')]);
    store.writeQueue(
        [OutboundCapture(idea: _idea('a'), idempotencyKey: 'key-a')]);
    store.writeWiki([_page('alpha')]);
    store.writeSyncMeta(cursor: 7, deviceId: 'd', siteId: 2, lamport: 9);

    store.clear();

    expect(store.readFeed(), isEmpty);
    expect(store.readQueue(), isEmpty);
    expect(store.readWiki(), isEmpty);
    expect(store.readSyncMeta(), isNull);
    store.dispose();

    // The wipe is durable: a reopen of the same on-disk DB is still empty.
    final reopened = SqliteCacheStore.open(dbPath);
    expect(reopened.readFeed(), isEmpty);
    expect(reopened.readQueue(), isEmpty);
    expect(reopened.readWiki(), isEmpty);
    expect(reopened.readSyncMeta(), isNull);
    reopened.dispose();
  });

  test('clear() wipes the in-memory store', () {
    final store = InMemoryCacheStore();
    store.writeFeed([_idea('a')]);
    store.writeQueue(
        [OutboundCapture(idea: _idea('a'), idempotencyKey: 'key-a')]);
    store.writeWiki([_page('alpha')]);
    store.writeSyncMeta(cursor: 1, deviceId: 'd', siteId: 2, lamport: 3);

    store.clear();

    expect(store.readFeed(), isEmpty);
    expect(store.readQueue(), isEmpty);
    expect(store.readWiki(), isEmpty);
    expect(store.readSyncMeta(), isNull);
  });
}
