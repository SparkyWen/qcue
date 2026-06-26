// QCue S4-R5: Dart models mirroring the Rust `protocol`/`store` wire shapes
// (snake_case JSON ↔ Dart enums). Field shapes mirror Appendix B; the enum
// wire tokens are verbatim from the §2.1 PG-enum prelude. In production this
// file is emitted by ts-rs/schemars codegen and CI-verified no-diff.
import 'package:collection/collection.dart';

T _enumFromJson<T extends Enum>(List<T> values, Map<T, String> wire, String s) {
  final hit = values.firstWhereOrNull((v) => wire[v] == s);
  if (hit == null) throw ArgumentError('unknown enum value: $s');
  return hit;
}

// ── ideas.ingest_state ──
enum IngestState { pending, ingesting, ingested, skippedRedundant, failed }
const _ingestWire = {
  IngestState.pending: 'pending',
  IngestState.ingesting: 'ingesting',
  IngestState.ingested: 'ingested',
  IngestState.skippedRedundant: 'skipped_redundant',
  IngestState.failed: 'failed',
};
IngestState ingestStateFromJson(String s) =>
    _enumFromJson(IngestState.values, _ingestWire, s);
String ingestStateToJson(IngestState v) => _ingestWire[v]!;

// ── ideas.kind ──
enum IdeaKind { text, voice, clip }
const _kindWire = {
  IdeaKind.text: 'text',
  IdeaKind.voice: 'voice',
  IdeaKind.clip: 'clip',
};
IdeaKind ideaKindFromJson(String s) =>
    _enumFromJson(IdeaKind.values, _kindWire, s);
String ideaKindToJson(IdeaKind v) => _kindWire[v]!;

// ── wiki_pages.type ──
enum WikiPageType {
  entity,
  concept,
  source,
  // `index` is reserved (Enum.index getter); the wire token stays 'index'.
  indexPage,
  log,
  contradiction,
  schema,
  comparison,
  overview,
}
const _wptWire = {
  WikiPageType.entity: 'entity',
  WikiPageType.concept: 'concept',
  WikiPageType.source: 'source',
  WikiPageType.indexPage: 'index',
  WikiPageType.log: 'log',
  WikiPageType.contradiction: 'contradiction',
  WikiPageType.schema: 'schema',
  WikiPageType.comparison: 'comparison',
  WikiPageType.overview: 'overview',
};
WikiPageType wikiPageTypeFromJson(String s) =>
    _enumFromJson(WikiPageType.values, _wptWire, s);
String wikiPageTypeToJson(WikiPageType v) => _wptWire[v]!;

/// Human-readable group label for a wiki page type (used as index headers).
String wikiPageTypeLabel(WikiPageType v) => switch (v) {
      WikiPageType.entity => 'Entity',
      WikiPageType.concept => 'Concept',
      WikiPageType.source => 'Source',
      WikiPageType.indexPage => 'Index',
      WikiPageType.log => 'Log',
      WikiPageType.contradiction => 'Contradiction',
      WikiPageType.schema => 'Schema',
      WikiPageType.comparison => 'Comparison',
      WikiPageType.overview => 'Overview',
    };

// ── jobs.state ──
enum JobState { pending, leased, done, failed, skipped, canceled }
const _jobWire = {
  JobState.pending: 'pending',
  JobState.leased: 'leased',
  JobState.done: 'done',
  JobState.failed: 'failed',
  JobState.skipped: 'skipped',
  JobState.canceled: 'canceled',
};
JobState jobStateFromJson(String s) =>
    _enumFromJson(JobState.values, _jobWire, s);
String jobStateToJson(JobState v) => _jobWire[v]!;

// ── jobs.kind ──
enum JobKind { ingest, lint, dream, transcribe, syncMaterialize, export }
const _jobKindWire = {
  JobKind.ingest: 'ingest',
  JobKind.lint: 'lint',
  JobKind.dream: 'dream',
  JobKind.transcribe: 'transcribe',
  JobKind.syncMaterialize: 'sync_materialize',
  JobKind.export: 'export',
};
JobKind jobKindFromJson(String s) =>
    _enumFromJson(JobKind.values, _jobKindWire, s);
String jobKindToJson(JobKind v) => _jobKindWire[v]!;

// ── approvals.status ──
enum ApprovalStatus { pending, approved, rejected, expired }
const _apprWire = {
  ApprovalStatus.pending: 'pending',
  ApprovalStatus.approved: 'approved',
  ApprovalStatus.rejected: 'rejected',
  ApprovalStatus.expired: 'expired',
};
ApprovalStatus approvalStatusFromJson(String s) =>
    _enumFromJson(ApprovalStatus.values, _apprWire, s);
String approvalStatusToJson(ApprovalStatus v) => _apprWire[v]!;

// ── provider_credentials.status ──
enum CredStatus { ok, exhausted, dead }
const _credWire = {
  CredStatus.ok: 'ok',
  CredStatus.exhausted: 'exhausted',
  CredStatus.dead: 'dead',
};
CredStatus credStatusFromJson(String s) =>
    _enumFromJson(CredStatus.values, _credWire, s);
String credStatusToJson(CredStatus v) => _credWire[v]!;

// ── Idea (mirrors Appendix B 4.7) ──
class Idea {
  const Idea({
    required this.id,
    required this.tenantId,
    required this.userId,
    required this.kind,
    required this.body,
    required this.origin,
    required this.ingestState,
    required this.capturedAt,
    this.sourceUrl,
    this.transcriptProvider,
    this.queued = false,
    this.lat,
    this.lng,
    this.locAccuracyM,
    this.sourcePageSlug,
  });

  final String id;
  final String tenantId;
  final String userId;
  final IdeaKind kind;
  final String body;
  final String origin; // capture|share|web|import|voice
  final IngestState ingestState;
  final DateTime capturedAt;
  final String? sourceUrl;
  final String? transcriptProvider;

  /// Local-only (never on the wire): true while this capture sits unflushed in
  /// the offline outbound queue (S4-R25). Drives the distinct "queued / will
  /// sync" feed dot; cleared once the server acks the capture.
  final bool queued;

  /// GPS latitude at capture time (optional; Task 9 / S4-R5).
  final double? lat;

  /// GPS longitude at capture time (optional; Task 9 / S4-R5).
  final double? lng;

  /// Horizontal accuracy in metres reported by the OS (optional; Task 9).
  final double? locAccuracyM;

  /// Wiki page slug this capture was captured from (optional; Task 9).
  final String? sourcePageSlug;

  factory Idea.fromJson(Map<String, dynamic> j) => Idea(
        id: j['id'] as String,
        tenantId: j['tenant_id'] as String,
        userId: j['user_id'] as String,
        kind: ideaKindFromJson(j['kind'] as String),
        body: j['body'] as String,
        origin: j['origin'] as String,
        ingestState: ingestStateFromJson(j['ingest_state'] as String),
        capturedAt: DateTime.parse(j['captured_at'] as String),
        sourceUrl: j['source_url'] as String?,
        transcriptProvider: j['transcript_provider'] as String?,
        lat: (j['lat'] as num?)?.toDouble(),
        lng: (j['lng'] as num?)?.toDouble(),
        locAccuracyM: (j['loc_accuracy_m'] as num?)?.toDouble(),
        sourcePageSlug: j['source_page_slug'] as String?,
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'tenant_id': tenantId,
        'user_id': userId,
        'kind': ideaKindToJson(kind),
        'body': body,
        'origin': origin,
        'ingest_state': ingestStateToJson(ingestState),
        'captured_at': capturedAt.toUtc().toIso8601String(),
        if (sourceUrl != null) 'source_url': sourceUrl,
        if (transcriptProvider != null) 'transcript_provider': transcriptProvider,
        if (lat != null) 'lat': lat,
        if (lng != null) 'lng': lng,
        if (locAccuracyM != null) 'loc_accuracy_m': locAccuracyM,
        if (sourcePageSlug != null) 'source_page_slug': sourcePageSlug,
      };

  Idea copyWith({
    IngestState? ingestState,
    bool? queued,
    String? body,
    double? lat,
    double? lng,
    double? locAccuracyM,
    String? sourcePageSlug,
  }) => Idea(
        id: id,
        tenantId: tenantId,
        userId: userId,
        kind: kind,
        body: body ?? this.body,
        origin: origin,
        ingestState: ingestState ?? this.ingestState,
        capturedAt: capturedAt,
        sourceUrl: sourceUrl,
        transcriptProvider: transcriptProvider,
        queued: queued ?? this.queued,
        lat: lat ?? this.lat,
        lng: lng ?? this.lng,
        locAccuracyM: locAccuracyM ?? this.locAccuracyM,
        sourcePageSlug: sourcePageSlug ?? this.sourcePageSlug,
      );

  @override
  bool operator ==(Object other) =>
      other is Idea &&
      other.id == id &&
      other.tenantId == tenantId &&
      other.userId == userId &&
      other.kind == kind &&
      other.body == body &&
      other.origin == origin &&
      other.ingestState == ingestState &&
      other.capturedAt == capturedAt &&
      other.sourceUrl == sourceUrl &&
      other.transcriptProvider == transcriptProvider &&
      other.queued == queued &&
      other.lat == lat &&
      other.lng == lng &&
      other.locAccuracyM == locAccuracyM &&
      other.sourcePageSlug == sourcePageSlug;

  @override
  int get hashCode => Object.hash(id, tenantId, userId, kind, body, origin,
      ingestState, capturedAt, sourceUrl, transcriptProvider, queued,
      lat, lng, locAccuracyM, sourcePageSlug);
}

// ── WikiLink + WikiPage (mirrors Appendix B 4.8/4.9) ──
class WikiLink {
  const WikiLink({required this.targetSlug, this.targetPageId, this.display});
  final String targetSlug;
  final String? targetPageId; // null ⇒ dead link
  final String? display;
  bool get isDead => targetPageId == null;

  factory WikiLink.fromJson(Map<String, dynamic> j) => WikiLink(
        targetSlug: j['target_slug'] as String,
        targetPageId: j['target_page_id'] as String?,
        display: j['display'] as String?,
      );
  Map<String, dynamic> toJson() => {
        'target_slug': targetSlug,
        if (targetPageId != null) 'target_page_id': targetPageId,
        if (display != null) 'display': display,
      };
  @override
  bool operator ==(Object other) =>
      other is WikiLink &&
      other.targetSlug == targetSlug &&
      other.targetPageId == targetPageId &&
      other.display == display;
  @override
  int get hashCode => Object.hash(targetSlug, targetPageId, display);
}

class WikiPage {
  const WikiPage({
    required this.id,
    required this.type,
    required this.slug,
    required this.title,
    required this.summary,
    required this.bodyMarkdown,
    required this.updated,
    this.aliases = const [],
    this.tags = const [],
    this.backlinks = const [],
  });

  final String id;
  final WikiPageType type;
  final String slug;
  final String title;
  final String summary;
  final String bodyMarkdown; // rendered from the .md body
  final DateTime updated;
  final List<String> aliases;
  final List<String> tags;
  final List<WikiLink> backlinks;

  factory WikiPage.fromJson(Map<String, dynamic> j) => WikiPage(
        id: j['id'] as String,
        type: wikiPageTypeFromJson(j['type'] as String),
        slug: j['slug'] as String,
        title: j['title'] as String,
        summary: j['summary'] as String? ?? '',
        bodyMarkdown: j['body_markdown'] as String? ?? '',
        updated: DateTime.parse(j['updated'] as String),
        aliases: (j['aliases'] as List?)?.cast<String>() ?? const [],
        tags: (j['tags'] as List?)?.cast<String>() ?? const [],
        backlinks: (j['backlinks'] as List?)
                ?.map((e) => WikiLink.fromJson(e as Map<String, dynamic>))
                .toList() ??
            const [],
      );
  Map<String, dynamic> toJson() => {
        'id': id,
        'type': wikiPageTypeToJson(type),
        'slug': slug,
        'title': title,
        'summary': summary,
        'body_markdown': bodyMarkdown,
        'updated': updated.toUtc().toIso8601String(),
        'aliases': aliases,
        'tags': tags,
        'backlinks': backlinks.map((b) => b.toJson()).toList(),
      };

  /// An index-row projection: title + summary only, body and backlinks dropped
  /// (the index list never needs the body; the page view fetches it).
  WikiPage copyWithoutBody() => WikiPage(
        id: id,
        type: type,
        slug: slug,
        title: title,
        summary: summary,
        bodyMarkdown: '',
        updated: updated,
        aliases: aliases,
        tags: tags,
      );
  @override
  bool operator ==(Object other) =>
      other is WikiPage &&
      other.id == id &&
      other.type == type &&
      other.slug == slug &&
      other.title == title &&
      other.summary == summary &&
      other.bodyMarkdown == bodyMarkdown &&
      other.updated == updated &&
      const ListEquality<String>().equals(other.aliases, aliases) &&
      const ListEquality<String>().equals(other.tags, tags) &&
      const ListEquality<WikiLink>().equals(other.backlinks, backlinks);
  @override
  int get hashCode => Object.hash(id, type, slug, title, summary, bodyMarkdown,
      updated, Object.hashAll(aliases), Object.hashAll(tags),
      Object.hashAll(backlinks));
}

// ── JobRow (mirrors Appendix B 4.15) ──
class JobRow {
  const JobRow({
    required this.id,
    required this.kind,
    required this.state,
    this.progress,
    this.lastError,
  });
  final String id;
  final JobKind kind;
  final JobState state;
  final double? progress; // 0..1 while leased
  final String? lastError;

  factory JobRow.fromJson(Map<String, dynamic> j) => JobRow(
        id: j['id'] as String,
        kind: jobKindFromJson(j['kind'] as String),
        state: jobStateFromJson(j['state'] as String),
        progress: (j['progress'] as num?)?.toDouble(),
        lastError: j['last_error'] as String?,
      );
  Map<String, dynamic> toJson() => {
        'id': id,
        'kind': jobKindToJson(kind),
        'state': jobStateToJson(state),
        if (progress != null) 'progress': progress,
        if (lastError != null) 'last_error': lastError,
      };
  @override
  bool operator ==(Object other) =>
      other is JobRow &&
      other.id == id &&
      other.kind == kind &&
      other.state == state &&
      other.progress == progress &&
      other.lastError == lastError;
  @override
  int get hashCode => Object.hash(id, kind, state, progress, lastError);
}

// ── Approval (mirrors Appendix B 4.19) ──
class Approval {
  const Approval({
    required this.id,
    required this.action, // wiki_merge|wiki_delete|schema_apply|external_send|paid_action
    required this.status,
    required this.requestedBy, // dream|ingest|lint|user
    required this.subjectRef,
  });
  final String id;
  final String action;
  final ApprovalStatus status;
  final String requestedBy;
  final Map<String, dynamic> subjectRef;

  bool get isDestructive => action == 'wiki_merge' || action == 'wiki_delete';

  factory Approval.fromJson(Map<String, dynamic> j) => Approval(
        id: j['id'] as String,
        action: j['action'] as String,
        status: approvalStatusFromJson(j['status'] as String),
        requestedBy: j['requested_by'] as String,
        subjectRef:
            (j['subject_ref'] as Map?)?.cast<String, dynamic>() ?? const {},
      );
  Map<String, dynamic> toJson() => {
        'id': id,
        'action': action,
        'status': approvalStatusToJson(status),
        'requested_by': requestedBy,
        'subject_ref': subjectRef,
      };
  @override
  bool operator ==(Object other) =>
      other is Approval &&
      other.id == id &&
      other.action == action &&
      other.status == status &&
      other.requestedBy == requestedBy &&
      const MapEquality<String, dynamic>().equals(other.subjectRef, subjectRef);
  @override
  int get hashCode => Object.hash(
      id, action, status, requestedBy, Object.hashAll(subjectRef.values));
}

// ── CostLedgerRow (mirrors Appendix B 4.18; 5-field CanonicalUsage) ──
class CostLedgerRow {
  const CostLedgerRow({
    required this.day,
    required this.inputTokens,
    required this.outputTokens,
    required this.cacheReadTokens,
    required this.cacheWriteTokens,
    required this.reasoningTokens,
    required this.costMicros,
  });
  final DateTime day;
  final int inputTokens;
  final int outputTokens;
  final int cacheReadTokens;
  final int cacheWriteTokens;
  final int reasoningTokens; // the 5th CanonicalUsage field
  final int costMicros;

  double get costUsd => costMicros / 1e6;

  factory CostLedgerRow.fromJson(Map<String, dynamic> j) => CostLedgerRow(
        day: DateTime.parse(j['day'] as String),
        inputTokens: j['input_tokens'] as int,
        outputTokens: j['output_tokens'] as int,
        cacheReadTokens: j['cache_read_tokens'] as int? ?? 0,
        cacheWriteTokens: j['cache_write_tokens'] as int? ?? 0,
        reasoningTokens: j['reasoning_tokens'] as int? ?? 0,
        costMicros: j['cost_micros'] as int,
      );
  Map<String, dynamic> toJson() => {
        'day': day.toUtc().toIso8601String(),
        'input_tokens': inputTokens,
        'output_tokens': outputTokens,
        'cache_read_tokens': cacheReadTokens,
        'cache_write_tokens': cacheWriteTokens,
        'reasoning_tokens': reasoningTokens,
        'cost_micros': costMicros,
      };
  @override
  bool operator ==(Object other) =>
      other is CostLedgerRow &&
      other.day == day &&
      other.inputTokens == inputTokens &&
      other.outputTokens == outputTokens &&
      other.cacheReadTokens == cacheReadTokens &&
      other.cacheWriteTokens == cacheWriteTokens &&
      other.reasoningTokens == reasoningTokens &&
      other.costMicros == costMicros;
  @override
  int get hashCode => Object.hash(day, inputTokens, outputTokens,
      cacheReadTokens, cacheWriteTokens, reasoningTokens, costMicros);
}

// ── ProviderCredential (mirrors Appendix B 4.6; key never in plaintext) ──
class ProviderCredential {
  const ProviderCredential({
    required this.provider,
    required this.keyHint,
    required this.status,
    this.cooldownUntil,
  });
  final String provider;
  final String keyHint; // last-4 only (S4-R46)
  final CredStatus status;
  final DateTime? cooldownUntil;

  factory ProviderCredential.fromJson(Map<String, dynamic> j) =>
      ProviderCredential(
        provider: j['provider'] as String,
        keyHint: j['key_hint'] as String,
        status: credStatusFromJson(j['status'] as String),
        cooldownUntil: j['cooldown_until'] == null
            ? null
            : DateTime.parse(j['cooldown_until'] as String),
      );
  Map<String, dynamic> toJson() => {
        'provider': provider,
        'key_hint': keyHint,
        'status': credStatusToJson(status),
        if (cooldownUntil != null)
          'cooldown_until': cooldownUntil!.toUtc().toIso8601String(),
      };
  @override
  bool operator ==(Object other) =>
      other is ProviderCredential &&
      other.provider == provider &&
      other.keyHint == keyHint &&
      other.status == status &&
      other.cooldownUntil == cooldownUntil;
  @override
  int get hashCode => Object.hash(provider, keyHint, status, cooldownUntil);
}
