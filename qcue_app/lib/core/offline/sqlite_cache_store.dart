// QCue S4-R25/R28: the persistent [CacheStore] backed by sqlite3 via dart:ffi
// (master §2.2: the sanctioned local SQLite). It stores the feed, the outbound
// queue, and the wiki read-cache as ordered JSON rows so the cache survives an
// app restart (an offline capture is durable, never just in RAM).
//
// HOST NOTE: this Linux host ships `libsqlite3.so.0` with NO unversioned
// `libsqlite3.so` symlink, so on Linux we open the versioned soname. On Android
// `sqlite3_flutter_libs` bundles SQLite and the default loader finds it. On
// iOS/macOS the `sqlite3_flutter_libs` bundled library, under Flutter's Swift
// Package Manager integration, fails to auto-initialise and segfaults inside
// `sqlite3_open_v2` (a null allocator) — so we explicitly open the OS-provided
// `libsqlite3.dylib` (always present, fully initialised; this cache only needs
// standard SQL). The override is keyed per-OS and idempotent.
//
// The whole-list `write*`/`read*` shape mirrors [InMemoryCacheStore]: [IdeaCache]
// owns all cache policy, the store is a dumb ordered-row bag. Lists here are
// small (a feed cap of ~200, a wiki cap of ~32, a short queue), so a full
// rewrite per mutation is cheap and keeps ordering exact.
import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:sqlite3/open.dart';
import 'package:sqlite3/sqlite3.dart';

import '../models/protocol_models.dart';
import 'idea_cache.dart';

bool _openOverridden = false;

/// Point the sqlite3 loader at a platform-appropriate library on the desktop
/// dev / CI hosts where the default lookup misses: on Linux the bare
/// `libsqlite3.so` dev symlink is often absent (open the versioned soname
/// `libsqlite3.so.0`); on Windows there is no standalone `sqlite3.dll`, so open
/// the OS-bundled `winsqlite3.dll`. Idempotent; a no-op on iOS (system SQLite is
/// statically linked) and Android (`sqlite3_flutter_libs` bundles it).
void _ensureHostSqliteOpen() {
  if (_openOverridden) return;
  if (Platform.isLinux) {
    open.overrideFor(
      OperatingSystem.linux,
      () => DynamicLibrary.open('libsqlite3.so.0'),
    );
    _openOverridden = true;
  } else if (Platform.isWindows) {
    open.overrideFor(
      OperatingSystem.windows,
      () => DynamicLibrary.open('winsqlite3.dll'),
    );
    _openOverridden = true;
  } else if (Platform.isIOS || Platform.isMacOS) {
    // Bypass the `sqlite3_flutter_libs` bundled library (which crashes under the
    // SPM build) and use the always-present, fully-initialised system SQLite.
    // NOTE: system SQLite ships without the FTS5/rtree extensions — a future D8
    // full-text wiki search must not assume them on iOS/macOS.
    open
      ..overrideFor(
        OperatingSystem.iOS,
        () => DynamicLibrary.open('libsqlite3.dylib'),
      )
      ..overrideFor(
        OperatingSystem.macOS,
        () => DynamicLibrary.open('libsqlite3.dylib'),
      );
    _openOverridden = true;
  }
}

class SqliteCacheStore implements CacheStore {
  SqliteCacheStore._(this._db);

  final Database _db;

  /// Open (or create) the cache database at [path] (`:memory:` for an ephemeral
  /// store). Creates the three ordered-row tables if absent.
  factory SqliteCacheStore.open(String path) {
    _ensureHostSqliteOpen();
    final db = sqlite3.open(path);
    db
      ..execute('PRAGMA journal_mode=WAL;')
      ..execute('CREATE TABLE IF NOT EXISTS feed ('
          'ord INTEGER PRIMARY KEY, id TEXT NOT NULL, json TEXT NOT NULL);')
      ..execute('CREATE TABLE IF NOT EXISTS queue ('
          'ord INTEGER PRIMARY KEY, id TEXT NOT NULL, '
          'idempotency_key TEXT NOT NULL, json TEXT NOT NULL);')
      // Task 11: the queued edit/delete mutations (ordered JSON rows), so an
      // offline edit/delete is durable across a restart and flushed on reconnect.
      ..execute('CREATE TABLE IF NOT EXISTS mutations ('
          'ord INTEGER PRIMARY KEY, id TEXT NOT NULL, json TEXT NOT NULL);')
      ..execute('CREATE TABLE IF NOT EXISTS wiki ('
          'ord INTEGER PRIMARY KEY, slug TEXT NOT NULL, json TEXT NOT NULL);')
      // Sync Phase 1 (Task 9): a tiny key/value table for the durable sync
      // bookkeeping (cursor + device/site + HLC lamport), stored as one JSON blob.
      ..execute('CREATE TABLE IF NOT EXISTS sync_meta ('
          'k TEXT PRIMARY KEY, v TEXT NOT NULL);');
    return SqliteCacheStore._(db);
  }

  // ── feed ──

  @override
  void writeFeed(List<Idea> rows) {
    _db.execute('DELETE FROM feed;');
    final stmt = _db.prepare('INSERT INTO feed (ord, id, json) VALUES (?,?,?)');
    for (var i = 0; i < rows.length; i++) {
      stmt.execute([i, rows[i].id, jsonEncode(_ideaToJson(rows[i]))]);
    }
    stmt.dispose();
  }

  @override
  List<Idea> readFeed() {
    final res = _db.select('SELECT json FROM feed ORDER BY ord ASC;');
    return [
      for (final r in res)
        _ideaFromJson(jsonDecode(r['json'] as String) as Map<String, dynamic>),
    ];
  }

  // ── outbound queue ──

  @override
  void writeQueue(List<OutboundCapture> rows) {
    _db.execute('DELETE FROM queue;');
    final stmt = _db.prepare(
        'INSERT INTO queue (ord, id, idempotency_key, json) VALUES (?,?,?,?)');
    for (var i = 0; i < rows.length; i++) {
      stmt.execute([
        i,
        rows[i].idea.id,
        rows[i].idempotencyKey,
        jsonEncode(_ideaToJson(rows[i].idea)),
      ]);
    }
    stmt.dispose();
  }

  @override
  List<OutboundCapture> readQueue() {
    final res = _db.select(
        'SELECT idempotency_key, json FROM queue ORDER BY ord ASC;');
    return [
      for (final r in res)
        OutboundCapture(
          idea: _ideaFromJson(
              jsonDecode(r['json'] as String) as Map<String, dynamic>),
          idempotencyKey: r['idempotency_key'] as String,
        ),
    ];
  }

  // ── edit/delete mutation queue (Task 11) ──

  @override
  void writeMutations(List<CaptureMutation> rows) {
    _db.execute('DELETE FROM mutations;');
    final stmt =
        _db.prepare('INSERT INTO mutations (ord, id, json) VALUES (?,?,?)');
    for (var i = 0; i < rows.length; i++) {
      stmt.execute([i, rows[i].id, jsonEncode(rows[i].toJson())]);
    }
    stmt.dispose();
  }

  @override
  List<CaptureMutation> readMutations() {
    final res = _db.select('SELECT json FROM mutations ORDER BY ord ASC;');
    return [
      for (final r in res)
        CaptureMutation.fromJson(
            jsonDecode(r['json'] as String) as Map<String, dynamic>),
    ];
  }

  // ── wiki read-cache ──

  @override
  void writeWiki(List<WikiPage> pages) {
    _db.execute('DELETE FROM wiki;');
    final stmt =
        _db.prepare('INSERT INTO wiki (ord, slug, json) VALUES (?,?,?)');
    for (var i = 0; i < pages.length; i++) {
      stmt.execute([i, pages[i].slug, jsonEncode(pages[i].toJson())]);
    }
    stmt.dispose();
  }

  @override
  List<WikiPage> readWiki() {
    final res = _db.select('SELECT json FROM wiki ORDER BY ord ASC;');
    return [
      for (final r in res)
        WikiPage.fromJson(
            jsonDecode(r['json'] as String) as Map<String, dynamic>),
    ];
  }

  // ── sync_meta (durable sync cursor + device/site + HLC lamport) ──

  static const _syncMetaKey = 'meta';
  static const _ownerKey = 'owner_user_id';

  @override
  String? readOwner() {
    final res = _db.select('SELECT v FROM sync_meta WHERE k=?;', [_ownerKey]);
    return res.isEmpty ? null : res.first['v'] as String;
  }

  @override
  void writeOwner(String userId) {
    _db.execute(
      'INSERT INTO sync_meta (k, v) VALUES (?, ?) '
      'ON CONFLICT(k) DO UPDATE SET v=excluded.v;',
      [_ownerKey, userId],
    );
  }

  @override
  void writeSyncMeta({
    required int cursor,
    required String deviceId,
    required int siteId,
    required int lamport,
  }) {
    final blob = jsonEncode(SyncMeta(
      cursor: cursor,
      deviceId: deviceId,
      siteId: siteId,
      lamport: lamport,
    ).toJson());
    _db.execute(
      'INSERT INTO sync_meta (k, v) VALUES (?, ?) '
      'ON CONFLICT(k) DO UPDATE SET v=excluded.v;',
      [_syncMetaKey, blob],
    );
  }

  @override
  SyncMeta? readSyncMeta() {
    final res =
        _db.select('SELECT v FROM sync_meta WHERE k=?;', [_syncMetaKey]);
    if (res.isEmpty) return null;
    return SyncMeta.fromJson(
        jsonDecode(res.first['v'] as String) as Map<String, dynamic>);
  }

  @override
  void clear() {
    _db
      ..execute('DELETE FROM feed;')
      ..execute('DELETE FROM queue;')
      ..execute('DELETE FROM mutations;')
      ..execute('DELETE FROM wiki;')
      ..execute('DELETE FROM sync_meta;');
  }

  void dispose() => _db.dispose();

  // Idea.toJson()/fromJson() drop the local-only `queued` flag (it is never on
  // the wire), so persist it alongside the wire JSON here to keep the cached
  // queued state durable across a reopen.
  Map<String, dynamic> _ideaToJson(Idea i) => {
        ...i.toJson(),
        'queued': i.queued,
      };

  Idea _ideaFromJson(Map<String, dynamic> j) {
    final idea = Idea.fromJson(j);
    return (j['queued'] as bool? ?? false)
        ? idea.copyWith(queued: true)
        : idea;
  }
}
