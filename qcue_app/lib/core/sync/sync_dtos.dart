// QCue Sync Phase 1 (Tasks 10/11): light hand-written Dart DTOs mirroring the
// Rust serde wire types in `qcue-rs/protocol/src/sync.rs` (JSON is snake_case).
// The backend `codegen/models.dart` isn't regenerated for sync yet, so these
// stay hand-written per the plan; they parse/emit the exact same shapes as the
// Rust `SyncOp`/`SyncDelta`/`SyncSnapshot`/`IdeaSnap`/`WikiPageSnap` types and the
// `/v1/sync/register` response.

/// The `/v1/sync/register` response: the per-tenant device + its HLC `site_id`
/// (device site_ids start at 1; the server reserves site_id 0 for its own ops).
class DeviceReg {
  const DeviceReg({required this.deviceId, required this.siteId});
  final String deviceId;
  final int siteId;

  factory DeviceReg.fromJson(Map<String, dynamic> j) => DeviceReg(
        deviceId: j['device_id'] as String,
        siteId: (j['site_id'] as num).toInt(),
      );
  Map<String, dynamic> toJson() => {
        'device_id': deviceId,
        'site_id': siteId,
      };
}

/// One HLC-stamped op (`SyncOp`). The HLC tuple `(hlc_wall_ms, hlc_lamport,
/// site_id)` totally orders; `op` is the opaque CRDT bag (op grammar §5):
///   idea       · {"create": {body, origin, captured_at}}
///   wiki_page  · {"create": {type}} | {"set_title": "<t>"} |
///                {"set_body": "<md>", "base_version": N} | {"delete": true}
class SyncOp {
  const SyncOp({
    required this.hlcWallMs,
    required this.hlcLamport,
    required this.siteId,
    required this.entityKind, // "idea" | "wiki_page"
    required this.entityRef, // client_uuid | slug
    required this.op,
  });

  final int hlcWallMs;
  final int hlcLamport;
  final int siteId;
  final String entityKind;
  final String entityRef;
  final Map<String, dynamic> op;

  factory SyncOp.fromJson(Map<String, dynamic> j) => SyncOp(
        hlcWallMs: (j['hlc_wall_ms'] as num).toInt(),
        hlcLamport: (j['hlc_lamport'] as num).toInt(),
        siteId: (j['site_id'] as num).toInt(),
        entityKind: j['entity_kind'] as String,
        entityRef: j['entity_ref'] as String,
        op: (j['op'] as Map?)?.cast<String, dynamic>() ?? const {},
      );
  Map<String, dynamic> toJson() => {
        'hlc_wall_ms': hlcWallMs,
        'hlc_lamport': hlcLamport,
        'site_id': siteId,
        'entity_kind': entityKind,
        'entity_ref': entityRef,
        'op': op,
      };
}

/// An idea (capture) in the cold-start snapshot (`IdeaSnap`).
class IdeaSnap {
  const IdeaSnap({
    required this.id,
    required this.body,
    required this.origin,
    required this.capturedAt,
  });
  final String id;
  final String body;
  final String origin;
  final String capturedAt; // ISO-8601 (kept as the wire string)

  factory IdeaSnap.fromJson(Map<String, dynamic> j) => IdeaSnap(
        id: j['id'] as String,
        body: j['body'] as String,
        origin: j['origin'] as String,
        capturedAt: j['captured_at'] as String,
      );
  Map<String, dynamic> toJson() => {
        'id': id,
        'body': body,
        'origin': origin,
        'captured_at': capturedAt,
      };
}

/// A wiki page in the cold-start snapshot (`WikiPageSnap`). Bodies are omitted
/// from the snapshot listing (SYNC-D6): the client fetches a body only for a page
/// whose `content_hash` it doesn't already hold.
class WikiPageSnap {
  const WikiPageSnap({
    required this.slug,
    required this.title,
    required this.contentHash,
    required this.syncVersion,
  });
  final String slug;
  final String title;
  final String contentHash;
  final int syncVersion;

  factory WikiPageSnap.fromJson(Map<String, dynamic> j) => WikiPageSnap(
        slug: j['slug'] as String,
        title: j['title'] as String,
        contentHash: j['content_hash'] as String? ?? '',
        syncVersion: (j['sync_version'] as num?)?.toInt() ?? 0,
      );
  Map<String, dynamic> toJson() => {
        'slug': slug,
        'title': title,
        'content_hash': contentHash,
        'sync_version': syncVersion,
      };
}

/// The cold-start snapshot of the canonical tables (`SyncSnapshot`).
class SyncSnapshot {
  const SyncSnapshot({required this.ideas, required this.wikiPages});
  final List<IdeaSnap> ideas;
  final List<WikiPageSnap> wikiPages;

  factory SyncSnapshot.fromJson(Map<String, dynamic> j) => SyncSnapshot(
        ideas: (j['ideas'] as List? ?? const [])
            .map((e) => IdeaSnap.fromJson((e as Map).cast<String, dynamic>()))
            .toList(),
        wikiPages: (j['wiki_pages'] as List? ?? const [])
            .map(
                (e) => WikiPageSnap.fromJson((e as Map).cast<String, dynamic>()))
            .toList(),
      );
  Map<String, dynamic> toJson() => {
        'ideas': ideas.map((e) => e.toJson()).toList(),
        'wiki_pages': wikiPages.map((e) => e.toJson()).toList(),
      };
}

/// The `/v1/sync/pull` response (`SyncDelta`): a [snapshot] on cold start
/// (`since` absent/0) OR incremental [ops] by `seq`, plus the next [cursor].
class SyncDelta {
  const SyncDelta({
    required this.cursor,
    this.snapshot,
    this.ops = const [],
  });
  final int cursor;
  final SyncSnapshot? snapshot;
  final List<SyncOp> ops;

  factory SyncDelta.fromJson(Map<String, dynamic> j) => SyncDelta(
        cursor: (j['cursor'] as num?)?.toInt() ?? 0,
        snapshot: j['snapshot'] == null
            ? null
            : SyncSnapshot.fromJson(
                (j['snapshot'] as Map).cast<String, dynamic>()),
        ops: (j['ops'] as List? ?? const [])
            .map((e) => SyncOp.fromJson((e as Map).cast<String, dynamic>()))
            .toList(),
      );
  Map<String, dynamic> toJson() => {
        'cursor': cursor,
        if (snapshot != null) 'snapshot': snapshot!.toJson(),
        if (ops.isNotEmpty) 'ops': ops.map((e) => e.toJson()).toList(),
      };
}
