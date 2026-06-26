// QCue Sync Phase 1 (Task 9): the sqlite cache gains a `sync_meta` key/value
// table that durably holds the sync cursor + device/site identity + the local
// HLC lamport. It must survive a reopen of the same on-disk database so a pulled
// cursor is not replayed from scratch after an app restart.
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';
import 'package:qcue_app/core/offline/sqlite_cache_store.dart';

void main() {
  late Directory dir;
  late String dbPath;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('qcue_sync_meta_test');
    dbPath = '${dir.path}/cache.db';
  });
  tearDown(() {
    if (dir.existsSync()) dir.deleteSync(recursive: true);
  });

  test('sync_meta persists cursor + device + site + lamport across a reopen', () {
    final store = SqliteCacheStore.open(dbPath);
    store.writeSyncMeta(cursor: 5, deviceId: 'dev-1', siteId: 1, lamport: 2);
    store.dispose();

    final reopened = SqliteCacheStore.open(dbPath);
    final meta = reopened.readSyncMeta();
    expect(meta, isNotNull);
    expect(meta!.cursor, 5);
    expect(meta.deviceId, 'dev-1');
    expect(meta.siteId, 1);
    expect(meta.lamport, 2);
    reopened.dispose();
  });

  test('readSyncMeta returns null on a fresh store (no prior write)', () {
    final store = SqliteCacheStore.open(dbPath);
    expect(store.readSyncMeta(), isNull);
    store.dispose();
  });

  test('writeSyncMeta overwrites the prior blob (latest wins)', () {
    final store = SqliteCacheStore.open(dbPath);
    store.writeSyncMeta(cursor: 1, deviceId: 'd', siteId: 1, lamport: 0);
    store.writeSyncMeta(cursor: 9, deviceId: 'd', siteId: 1, lamport: 7);
    final meta = store.readSyncMeta();
    expect(meta!.cursor, 9);
    expect(meta.lamport, 7);
    store.dispose();
  });

  test('InMemoryCacheStore mirrors the sync_meta seam', () {
    final store = InMemoryCacheStore();
    expect(store.readSyncMeta(), isNull);
    store.writeSyncMeta(cursor: 3, deviceId: 'mem', siteId: 4, lamport: 5);
    final meta = store.readSyncMeta();
    expect(meta!.cursor, 3);
    expect(meta.deviceId, 'mem');
    expect(meta.siteId, 4);
    expect(meta.lamport, 5);
  });
}
