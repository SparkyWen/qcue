// QCue Sync Phase 1 (Task 10): the read-sync client. It registers this device,
// pulls the change feed from the server (`GET /v1/sync/pull?since=<seq>`), and
// applies the delta into the existing offline cache so the Capture feed + Wiki
// browser — which already render offline-first from that cache — surface a
// change made on (or by the server for) another device after a pull.
//
// SHAPE
//   - register(): registers the device (persists device_id + HLC site_id into
//     the cache's sync_meta) — idempotent server-side.
//   - pull(): reads the persisted cursor; pulls `since=<cursor>`; a cold pull
//     (cursor 0) applies a SNAPSHOT (SYNC-D5) — ideas via putFeed (preserving any
//     unflushed queued captures, exactly like the read path) + wiki via the index
//     projection; a warm pull applies incremental OPS by a local LWW reducer
//     (SYNC-D4 — the server already ordered them by seq). The new cursor is then
//     persisted so the next pull resumes incrementally and never replays.
//
// The reducer is LWW-trivial in Phase 1 (the server is the single ordering
// authority); local outbound ops are a Phase 2 concern. This client never writes
// wiki markdown anywhere but the read-cache — it is a projection, not the vault.
import '../models/protocol_models.dart';
import '../offline/idea_cache.dart';
import 'sync_dtos.dart';

class SyncClient {
  SyncClient({
    required Future<DeviceReg> Function(String platform) registerDevice,
    required Future<SyncDelta> Function({required int since}) pullSync,
    required this.cache,
    required this.platform,
    // The fields are renamed (private), so initializing formals can't apply.
    // ignore: prefer_initializing_formals
  })  : _registerDevice = registerDevice,
        // ignore: prefer_initializing_formals
        _pullSync = pullSync;

  final Future<DeviceReg> Function(String platform) _registerDevice;
  final Future<SyncDelta> Function({required int since}) _pullSync;
  final IdeaCache cache;

  /// The OS platform tag sent on register (`android` | `ios` | …).
  final String platform;

  /// Register this device with the tenant and persist its identity (device_id +
  /// HLC site_id) into the cache's sync_meta, preserving the existing cursor +
  /// local lamport. Idempotent — safe to call on every boot.
  Future<void> register() async {
    final reg = await _registerDevice(platform);
    final prior = cache.syncMeta();
    cache.writeSyncMeta(SyncMeta(
      cursor: prior?.cursor ?? 0,
      deviceId: reg.deviceId,
      siteId: reg.siteId,
      lamport: prior?.lamport ?? 0,
    ));
  }

  /// Pull the change feed since the persisted cursor and apply it into the cache.
  /// Cold (cursor 0) → snapshot bootstrap; warm → incremental ops. Persists the
  /// returned cursor. Safe to call repeatedly (on start/resume/online/periodic);
  /// a network error propagates to the caller (the triggers swallow it).
  ///
  /// Returns `true` when the delta actually changed the cache (a snapshot, or any
  /// ops) so the caller can refresh the read-providers ONLY on real change — an
  /// empty warm delta returns `false`, avoiding a wasted re-fetch every cadence.
  Future<bool> pull() async {
    final meta = cache.syncMeta();
    final since = meta?.cursor ?? 0;
    final delta = await _pullSync(since: since);
    return applyDelta(delta);
  }

  /// Apply a [SyncDelta] into the cache (the local LWW reducer). A snapshot is a
  /// full bootstrap; ops are incremental. Then persist the new cursor. Returns
  /// whether anything was applied (snapshot present, or a non-empty op batch).
  bool applyDelta(SyncDelta delta) {
    final snapshot = delta.snapshot;
    final changed = snapshot != null || delta.ops.isNotEmpty;
    if (snapshot != null) {
      _applySnapshot(snapshot);
    } else {
      for (final op in delta.ops) {
        _applyOp(op);
      }
    }
    _persistCursor(delta.cursor);
    return changed;
  }

  // ── snapshot bootstrap ──

  void _applySnapshot(SyncSnapshot snapshot) {
    // Ideas: reconcile the feed from the server snapshot while NEVER dropping an
    // unflushed queued capture the server hasn't acked yet (putFeed's contract).
    cache.putFeed([for (final s in snapshot.ideas) _ideaFromSnap(s)]);
    // Wiki: the snapshot lists pages without bodies (SYNC-D6) — cache the index
    // projection (existing fully-cached bodies are preserved by putWikiIndex).
    cache.putWikiIndex([for (final p in snapshot.wikiPages) _pageFromSnap(p)]);
  }

  // ── incremental ops (op grammar §5) ──

  void _applyOp(SyncOp op) {
    switch (op.entityKind) {
      case 'idea':
        final create = op.op['create'];
        if (create is Map) {
          cache.putFeedRow(_ideaFromOp(op.entityRef, create.cast<String, dynamic>()));
        }
      case 'wiki_page':
        _applyWikiOp(op);
    }
    // Unknown entity kinds / op keys are ignored (forward-compat, op grammar §5).
  }

  void _applyWikiOp(SyncOp op) {
    final slug = op.entityRef;
    final prior = cache.wikiPage(slug);
    final setTitle = op.op['set_title'];
    final setBody = op.op['set_body'];
    if (op.op['delete'] == true) {
      // A tombstone — Phase 1 surfaces the change minimally by clearing the body.
      // (A soft-delete UX is Phase 3; here we just stop serving a stale body.)
      if (prior != null) {
        cache.putWikiPage(_withFields(prior, body: ''));
      }
      return;
    }
    if (setTitle == null && setBody == null) return; // create-only / no-op
    final base = prior ??
        WikiPage(
          id: slug,
          type: WikiPageType.concept,
          slug: slug,
          title: slug,
          summary: '',
          bodyMarkdown: '',
          updated: DateTime.now().toUtc(),
        );
    cache.putWikiPage(_withFields(
      base,
      title: setTitle is String ? setTitle : null,
      body: setBody is String ? setBody : null,
    ));
  }

  // ── persistence ──

  void _persistCursor(int cursor) {
    final prior = cache.syncMeta();
    cache.writeSyncMeta(SyncMeta(
      cursor: cursor,
      deviceId: prior?.deviceId ?? '',
      siteId: prior?.siteId ?? 0,
      lamport: prior?.lamport ?? 0,
    ));
  }

  // ── snap/op → model mappers ──

  Idea _ideaFromSnap(IdeaSnap s) => Idea(
        id: s.id,
        tenantId: '',
        userId: '',
        kind: _kindFor(s.origin),
        body: s.body,
        origin: s.origin,
        // A synced server row is already ingested as far as this device knows.
        ingestState: IngestState.ingested,
        capturedAt: DateTime.parse(s.capturedAt),
      );

  Idea _ideaFromOp(String id, Map<String, dynamic> create) => Idea(
        id: id,
        tenantId: '',
        userId: '',
        kind: _kindFor(create['origin'] as String? ?? 'capture'),
        body: create['body'] as String? ?? '',
        origin: create['origin'] as String? ?? 'capture',
        ingestState: IngestState.ingested,
        capturedAt: create['captured_at'] != null
            ? DateTime.parse(create['captured_at'] as String)
            : DateTime.now().toUtc(),
      );

  WikiPage _pageFromSnap(WikiPageSnap p) => WikiPage(
        id: p.slug,
        type: WikiPageType.concept,
        slug: p.slug,
        title: p.title,
        summary: '',
        bodyMarkdown: '', // bodies omitted from the snapshot (SYNC-D6)
        updated: DateTime.now().toUtc(),
      );

  /// A best-effort kind from the capture origin (the snapshot/op carries origin,
  /// not kind); `voice` → voice, everything else → text.
  static IdeaKind _kindFor(String origin) =>
      origin == 'voice' ? IdeaKind.voice : IdeaKind.text;

  WikiPage _withFields(WikiPage p, {String? title, String? body}) => WikiPage(
        id: p.id,
        type: p.type,
        slug: p.slug,
        title: title ?? p.title,
        summary: p.summary,
        bodyMarkdown: body ?? p.bodyMarkdown,
        updated: DateTime.now().toUtc(),
        aliases: p.aliases,
        tags: p.tags,
        backlinks: p.backlinks,
      );
}
