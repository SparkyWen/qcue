// QCue S4-R25/R26/R28 (master §10 offline-capable; D5/D6 local-first): the
// offline read-cache + outbound capture queue.
//
// CANONICAL GUARANTEES
//   - A capture is persisted LOCALLY (feed row + outbound queue) BEFORE any
//     network attempt — an idea is never lost, even fully offline (D5/D6).
//   - The outbound flush is IDEMPOTENT by a client-generated id: each queued
//     capture carries a stable idempotency key (uuidv7-shaped); a retry POSTs at
//     most once and the server dedups by that key, so a double reconnect never
//     double-inserts.
//   - Reads degrade gracefully: the feed + last-opened wiki pages render from
//     the cache when the network is unavailable.
//   - LRU eviction trims old READ rows under a cap, but NEVER evicts an
//     unflushed queued capture.
//
// The persistent backing is sqlite3-via-dart:ffi ([SqliteCacheStore]); tests use
// the fast [InMemoryCacheStore]. Both implement the [CacheStore] seam, so the
// cache logic here is identical on-device and under `flutter test`. (The plan's
// note about rusqlite-via-FFI still holds for the eventual on-device crate; the
// seam is unchanged when that lands.)
import '../models/protocol_models.dart';

/// One queued capture with its idempotency key (uuidv7-shaped) so a retry never
/// double-inserts (S4-R26). Mirrors the server-side capture-dedup key.
class OutboundCapture {
  OutboundCapture({required this.idea, required this.idempotencyKey});
  final Idea idea;
  final String idempotencyKey;
}

/// A queued offline mutation of an existing capture (CAP-R2/R3, offline-capable C7).
/// Task 11: edits/deletes are buffered locally and flushed on reconnect with a
/// delete-wins / last-write-wins collapse, so a flurry of offline edits + a final
/// delete reach the server as a single delete.
class CaptureMutation {
  CaptureMutation({required this.id, required this.kind, this.body, this.lat, this.lng, this.locAccuracyM});
  final String id;
  final String kind; // 'edit' | 'delete'
  final String? body;
  final double? lat;
  final double? lng;
  final double? locAccuracyM;

  Map<String, dynamic> toJson() => {
        'id': id, 'kind': kind,
        if (body != null) 'body': body,
        if (lat != null) 'lat': lat,
        if (lng != null) 'lng': lng,
        if (locAccuracyM != null) 'loc_accuracy_m': locAccuracyM,
      };
  factory CaptureMutation.fromJson(Map<String, dynamic> j) => CaptureMutation(
        id: j['id'] as String, kind: j['kind'] as String,
        body: j['body'] as String?,
        lat: (j['lat'] as num?)?.toDouble(), lng: (j['lng'] as num?)?.toDouble(),
        locAccuracyM: (j['loc_accuracy_m'] as num?)?.toDouble(),
      );
}

/// Sync Phase 1 (Task 9): the durable sync bookkeeping the [SyncClient] persists
/// — the monotonic pull [cursor] (the server `seq` last applied), the registered
/// [deviceId]/[siteId], and the local HLC [lamport]. Stored as a small JSON blob
/// in the cache's `sync_meta` table so a pull never replays from scratch after a
/// restart.
class SyncMeta {
  const SyncMeta({
    required this.cursor,
    required this.deviceId,
    required this.siteId,
    required this.lamport,
  });
  final int cursor;
  final String deviceId;
  final int siteId;
  final int lamport;

  factory SyncMeta.fromJson(Map<String, dynamic> j) => SyncMeta(
        cursor: (j['cursor'] as num?)?.toInt() ?? 0,
        deviceId: j['device_id'] as String? ?? '',
        siteId: (j['site_id'] as num?)?.toInt() ?? 0,
        lamport: (j['lamport'] as num?)?.toInt() ?? 0,
      );
  Map<String, dynamic> toJson() => {
        'cursor': cursor,
        'device_id': deviceId,
        'site_id': siteId,
        'lamport': lamport,
      };
}

/// Backing-store seam. Production impl is sqlite3-via-ffi ([SqliteCacheStore]);
/// tests use [InMemoryCacheStore]. The store is a dumb row bag — all the cache
/// policy (LRU, queue protection, reconciliation) lives in [IdeaCache].
abstract interface class CacheStore {
  void writeFeed(List<Idea> rows);
  List<Idea> readFeed();
  void writeQueue(List<OutboundCapture> rows);
  List<OutboundCapture> readQueue();
  void writeWiki(List<WikiPage> pages);
  List<WikiPage> readWiki();

  /// Task 11: persist the queued edit/delete mutations (ordered) so an offline
  /// edit/delete survives an app restart and is flushed on the next reconnect.
  void writeMutations(List<CaptureMutation> rows);
  List<CaptureMutation> readMutations();

  /// Sync Phase 1 (Task 9): persist the durable sync bookkeeping (cursor +
  /// device/site + HLC lamport) as a small JSON blob; `null` until first write.
  void writeSyncMeta({
    required int cursor,
    required String deviceId,
    required int siteId,
    required int lamport,
  });
  SyncMeta? readSyncMeta();

  /// Drop ALL cached rows (feed + outbound queue + wiki + sync meta). Used when
  /// the account is deleted so no residual data survives on the device.
  void clear();

  /// ISO-R1: the user id (JWT `sub`) this cache belongs to, or null if never set.
  String? readOwner();
  void writeOwner(String userId);
}

/// An in-memory [CacheStore] for tests + a sane default before the persistent
/// store is injected at bootstrap.
class InMemoryCacheStore implements CacheStore {
  List<Idea> _feed = [];
  List<OutboundCapture> _queue = [];
  List<CaptureMutation> _mutations = [];
  List<WikiPage> _wiki = [];
  SyncMeta? _syncMeta;
  String? _owner;

  @override
  void writeFeed(List<Idea> rows) => _feed = List.of(rows);
  @override
  List<Idea> readFeed() => List.of(_feed);
  @override
  void writeQueue(List<OutboundCapture> rows) => _queue = List.of(rows);
  @override
  List<OutboundCapture> readQueue() => List.of(_queue);
  @override
  void writeMutations(List<CaptureMutation> rows) => _mutations = List.of(rows);
  @override
  List<CaptureMutation> readMutations() => List.of(_mutations);
  @override
  void writeWiki(List<WikiPage> pages) => _wiki = List.of(pages);
  @override
  List<WikiPage> readWiki() => List.of(_wiki);
  @override
  void writeSyncMeta({
    required int cursor,
    required String deviceId,
    required int siteId,
    required int lamport,
  }) =>
      _syncMeta = SyncMeta(
        cursor: cursor,
        deviceId: deviceId,
        siteId: siteId,
        lamport: lamport,
      );
  @override
  SyncMeta? readSyncMeta() => _syncMeta;
  @override
  void clear() {
    _feed = [];
    _queue = [];
    _mutations = [];
    _wiki = [];
    _syncMeta = null;
    _owner = null;
  }

  @override
  String? readOwner() => _owner;
  @override
  void writeOwner(String userId) => _owner = userId;
}

/// A monotonic uuidv7-shaped id (time-ordered, so lexical sort == chronological).
/// Replaced by the Rust uuidv7 at the FFI boundary in production; this is stable
/// and unique enough for a client-generated capture/idempotency key.
int _counter = 0;
String uuidv7() {
  final ms = DateTime.now().toUtc().millisecondsSinceEpoch;
  final seq = (_counter++) & 0xffffff;
  String hex(int v, int width) => v.toRadixString(16).padLeft(width, '0');
  final timeHi = hex((ms >> 16) & 0xffffffff, 8);
  final timeLo = hex(ms & 0xffff, 4);
  return '$timeHi-$timeLo-7${hex(seq, 6)}';
}

/// The offline read-cache + outbound capture queue (master §10 / D5/D6).
class IdeaCache {
  IdeaCache(this._store, {required this.feedCap, this.wikiCap = 32});

  final CacheStore _store;

  /// Max cached READ rows in the feed (queued captures are never counted out).
  final int feedCap;

  /// Max cached wiki pages (last-opened; LRU).
  final int wikiCap;

  // ── feed read-cache ──

  List<Idea> feed() => _store.readFeed();
  List<OutboundCapture> outbound() => _store.readQueue();

  /// Wipe every cached row (feed + queue + wiki + sync meta). Invoked on account
  /// deletion so a deleted account leaves no residual local data on the device.
  void clear() => _store.clear();

  /// ISO-R1: the account this cache currently belongs to (null until first adopt).
  String? owner() => _store.readOwner();

  /// ISO-R2: claim this cache for [userId]. If a DIFFERENT account owned it, wipe everything first so
  /// no prior-account data survives. Returns true iff it wiped. A no-op when the owner already matches.
  /// When there is no prior owner (first adopt), just stamps the owner and returns false (nothing to wipe).
  ///
  /// Migration guard: a pre-feature cache has no owner tag but may hold another account's residual
  /// data (old account-switch bug). An untagged-but-populated cache is treated as unknown ownership
  /// and wiped to prevent cross-account data leaks (ISO-R2 hardening).
  bool adoptOwner(String userId) {
    final prior = _store.readOwner();
    if (prior == userId) return false;
    // Wipe when the owner differs AND (there was a known prior owner OR the cache already holds data).
    // This guards against the migration case: a pre-feature cache with no owner tag but residual data
    // from a different account (old account-switch bug) would leak without this check.
    final hasData = _store.readFeed().isNotEmpty ||
        _store.readQueue().isNotEmpty ||
        _store.readWiki().isNotEmpty;
    if (prior != null || hasData) {
      _store.clear();        // drops feed/queue/mutations/wiki/sync_meta (incl. the old owner row)
      _store.writeOwner(userId);
      return true;
    }
    _store.writeOwner(userId);
    return false;
  }

  // ── sync bookkeeping (cursor + device/site + HLC) ──

  /// The durable sync bookkeeping (Task 9/10), or null before first write.
  SyncMeta? syncMeta() => _store.readSyncMeta();

  /// Persist the durable sync bookkeeping (the SyncClient owns the values).
  void writeSyncMeta(SyncMeta meta) => _store.writeSyncMeta(
        cursor: meta.cursor,
        deviceId: meta.deviceId,
        siteId: meta.siteId,
        lamport: meta.lamport,
      );

  /// True if [id] is a still-unflushed locally-queued capture (drives the
  /// distinct "queued / will sync" feed dot).
  bool isQueued(String id) =>
      _store.readQueue().any((q) => q.idea.id == id);

  /// The idempotency key stamped on the queued capture [id] at enqueue time, or
  /// null if no queued row matches (already flushed/reconciled). The immediate
  /// capture POST reuses this so a later flush of the same row dedups server-side
  /// (Task 6).
  String? idempotencyKeyFor(String id) {
    for (final q in _store.readQueue()) {
      if (q.idea.id == id) return q.idempotencyKey;
    }
    return null;
  }

  /// S4-R25: write the capture to the feed + the outbound queue IMMEDIATELY,
  /// before any network. Returns the queued (pending, locally-`queued`) Idea.
  ///
  /// LOC-R1: an optional action-time location (when the caller supplies it) is
  /// stamped on the queued Idea; because the queue serializes the full Idea JSON
  /// (which carries lat/lng/loc_accuracy_m), it survives the queue round-trip and
  /// is re-sent on flush.
  ///
  /// Part F / LOC-R3: [capturedAt] is the PRECISE action-time instant. The funnel
  /// computes it ONCE and passes it here so the queued Idea's `capturedAt` is the
  /// time the capture was MADE (not the time it was enqueued/flushed); it
  /// survives the queue round-trip and is re-sent on flush. Falls back to now()
  /// when null so existing callers are unchanged.
  Idea enqueueCapture({
    required String body,
    required String origin,
    double? lat,
    double? lng,
    double? accuracyM,
    DateTime? capturedAt,
  }) {
    final idea = Idea(
      id: uuidv7(),
      tenantId: 'local',
      userId: 'local',
      kind: IdeaKind.text,
      body: body,
      origin: origin,
      ingestState: IngestState.pending,
      capturedAt: capturedAt ?? DateTime.now().toUtc(),
      queued: true,
      lat: lat,
      lng: lng,
      locAccuracyM: accuracyM,
    );
    _store.writeQueue([
      ..._store.readQueue(),
      OutboundCapture(idea: idea, idempotencyKey: uuidv7()),
    ]);
    _putFeedRowInternal(idea);
    return idea;
  }

  /// Insert/refresh a single feed row (newest-first) under the LRU cap.
  void putFeedRow(Idea idea) => _putFeedRowInternal(idea);

  /// Reconcile the whole feed from a server read while NEVER dropping an
  /// unflushed queued capture the server hasn't acked yet.
  void putFeed(List<Idea> rows) {
    final queued = _queuedRows();
    final serverIds = rows.map((r) => r.id).toSet();
    final survivors = [
      for (final q in queued)
        if (!serverIds.contains(q.id)) q,
    ];
    // queued (unacked) rows first (newest), then the server rows, capped.
    final merged = [...survivors, ...rows];
    _store.writeFeed(_capFeed(merged));
  }

  void _putFeedRowInternal(Idea idea) {
    final rows = [idea, ..._store.readFeed().where((r) => r.id != idea.id)];
    _store.writeFeed(_capFeed(rows));
  }

  /// Apply the feed cap, but keep every still-queued capture regardless of cap
  /// (S4-R28: an unflushed capture is never evicted).
  List<Idea> _capFeed(List<Idea> rows) {
    final queuedIds = _store.readQueue().map((q) => q.idea.id).toSet();
    final kept = <Idea>[];
    var reads = 0;
    for (final r in rows) {
      if (queuedIds.contains(r.id)) {
        kept.add(r);
      } else if (reads < feedCap) {
        kept.add(r);
        reads++;
      }
    }
    return kept;
  }

  List<Idea> _queuedRows() {
    final feed = _store.readFeed();
    return [
      for (final q in _store.readQueue())
        feed.firstWhere((r) => r.id == q.idea.id, orElse: () => q.idea),
    ];
  }

  /// S4-R26: POST each queued capture once (the idempotency key dedups on the
  /// server); on ack, dequeue and flip the cached row to `ingested` (clearing
  /// the local `queued` flag). A throwing POST leaves THAT capture (and the rest)
  /// queued for a later retry and the flush returns normally — so it is safe to
  /// call repeatedly on reconnect/resume without surfacing a network error.
  Future<void> flush(Future<void> Function(OutboundCapture) post) async {
    for (final c in List<OutboundCapture>.of(_store.readQueue())) {
      try {
        await post(c);
      } catch (_) {
        // still offline / server rejected: keep this + the remaining captures
        // queued and stop; the next flush retries from here.
        return;
      }
      // dequeue this capture
      _store.writeQueue(
          _store.readQueue().where((q) => q.idea.id != c.idea.id).toList());
      // flip the cached feed row: server-acked, no longer locally queued
      final flipped =
          c.idea.copyWith(ingestState: IngestState.ingested, queued: false);
      _store.writeFeed(
        _store.readFeed().map((r) => r.id == flipped.id ? flipped : r).toList(),
      );
    }
  }

  /// Reconcile a single just-POSTed capture: drop its outbound entry and replace
  /// its provisional local feed row (keyed by [localId]) with the authoritative
  /// [serverIdea]. Used by the online capture path so the feed shows the server
  /// row (its real id, no longer `queued`) the instant the POST returns.
  void reconcileQueued(String localId, Idea serverIdea) {
    _store.writeQueue(
        _store.readQueue().where((q) => q.idea.id != localId).toList());
    final replaced = serverIdea.copyWith(queued: false);
    final rows = [
      replaced,
      ..._store.readFeed().where((r) => r.id != localId && r.id != replaced.id),
    ];
    _store.writeFeed(_capFeed(rows));
  }

  // ── edit/delete mutation queue (Task 11; delete-wins / last-write-wins) ──

  /// The queued edit/delete mutations (newest collapse applied), exposed so the
  /// bootstrap/flush can read them.
  List<CaptureMutation> mutations() => _store.readMutations();

  /// Queue an edit; apply optimistically to the cached feed. Collapses with any
  /// existing queued mutation for the same id (last edit wins; a queued delete
  /// absorbs the edit — once a row is slated for deletion an offline edit is moot).
  void enqueueEdit(String id, {String? body, double? lat, double? lng, double? locAccuracyM}) {
    final rows = _store.readMutations();
    final hasDelete = rows.any((m) => m.id == id && m.kind == 'delete');
    if (!hasDelete) {
      final next = rows.where((m) => !(m.id == id && m.kind == 'edit')).toList()
        ..add(CaptureMutation(id: id, kind: 'edit', body: body, lat: lat, lng: lng, locAccuracyM: locAccuracyM));
      _store.writeMutations(next);
      // optimistic feed update
      final feed = _store.readFeed().map((r) =>
          r.id == id ? r.copyWith(body: body, lat: lat, lng: lng, locAccuracyM: locAccuracyM) : r).toList();
      _store.writeFeed(feed);
    }
  }

  /// Queue a delete; drop any queued edit for the same id (delete wins) and
  /// remove it from the cached feed so the row disappears immediately offline.
  void enqueueDelete(String id) {
    final rows = _store.readMutations().where((m) => m.id != id).toList()
      ..add(CaptureMutation(id: id, kind: 'delete'));
    _store.writeMutations(rows);
    _store.writeFeed(_store.readFeed().where((r) => r.id != id).toList());
  }

  /// Drop a single queued mutation matching [id]+[kind]. Used by the online
  /// success path so a clean edit/delete isn't re-flushed later (Task 11).
  void dropMutation(String id, String kind) {
    _store.writeMutations(
        _store.readMutations().where((m) => !(m.id == id && m.kind == kind)).toList());
  }

  /// Flush queued mutations once each (idempotent; on a throw, keep the rest
  /// queued and stop — the next flush retries from here).
  Future<void> flushMutations(Future<void> Function(CaptureMutation) post) async {
    for (final m in List<CaptureMutation>.of(_store.readMutations())) {
      try {
        await post(m);
      } catch (_) {
        return;
      }
      _store.writeMutations(_store.readMutations().where((q) => !(q.id == m.id && q.kind == m.kind)).toList());
    }
  }

  // ── wiki read-cache (last-opened pages, LRU) ──

  /// Cache a last-opened wiki page (full body) for offline rendering.
  void putWikiPage(WikiPage page) {
    final rows = [
      page,
      ..._store.readWiki().where((p) => p.slug != page.slug),
    ];
    _store.writeWiki(rows.take(wikiCap).toList());
  }

  /// Cache an index read (title/summary projections; bodies dropped). Existing
  /// fully-cached pages keep their body so the page view still renders offline.
  void putWikiIndex(List<WikiPage> pages) {
    final existing = {for (final p in _store.readWiki()) p.slug: p};
    final merged = <WikiPage>[];
    for (final p in pages) {
      final prior = existing[p.slug];
      merged.add(prior != null && prior.bodyMarkdown.isNotEmpty ? prior : p);
    }
    _store.writeWiki(merged.take(wikiCap).toList());
  }

  WikiPage? wikiPage(String slug) {
    for (final p in _store.readWiki()) {
      if (p.slug == slug) return p;
    }
    return null;
  }

  /// The cached index projection (title/summary only; body dropped).
  List<WikiPage> wikiIndex() =>
      _store.readWiki().map((p) => p.copyWithoutBody()).toList();
}
