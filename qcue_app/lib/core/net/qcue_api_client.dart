// QCue S4 seam: the transport interface the SSE/WSS milestone implements.
// This foundation milestone defines the interface + a seeded stub so the app
// compiles and the 3 content screens (Capture/Wiki/Recall) wire to ONE data
// seam; the real WSS JSON-RPC-lite + SSE client (replay-on-reconnect, JWT-only
// auth) replaces [StubApiClient] wholesale in the next milestone.
import 'dart:async';

import '../models/app_release_manifest.dart';
import '../models/protocol_models.dart';
import '../models/recall_conversation.dart';
import '../models/runtime_event.dart';
import '../models/sse_event.dart';
import '../sync/sync_dtos.dart';
import 'capture_query.dart';

// D4: the typed transcription failure thrown by [transcribe] lives in core/models so UI can catch it
// without importing the transport layer (S4-R1); re-exported here so seam consumers see it too.
export '../models/transcribe_error.dart';

/// Connection lifecycle states surfaced to the UI (status dot, offline banner).
enum ApiConnectionState { disconnected, connecting, connected, reconnecting }

/// The STT picker payload from `GET /v1/transcribe/providers` (D4 multi-provider voice):
/// [available] = the tenant's configured STT-capable BYOK providers (auto-derive priority order);
/// [allCapable] = every STT-capable vendor; [selected] = the explicit choice (null ⇒ "Auto").
class SttProviders {
  const SttProviders({
    required this.selected,
    required this.available,
    required this.allCapable,
  });
  final String? selected;
  final List<String> available;
  final List<String> allCapable;
}

/// The narrow transport surface the rest of the app depends on. Implementations:
///  - WssApiClient (JSON-RPC-lite, no `jsonrpc` field; sends the JWT only).
///  - SseApiClient (`?token=` auth; replay-on-reconnect via ring-buffer offset;
///    unknown-event skip — forward-compat via [RuntimeEventEnvelope]).
abstract interface class QcueApiClient {
  /// The live connection state (drives the status dot / offline banner).
  Stream<ApiConnectionState> get connectionState;

  /// The ordered runtime-event stream for a thread (replayed on reconnect).
  /// Envelopes are forward-compatible: unknown `event` strings still arrive.
  Stream<RuntimeEventEnvelope> events({required String threadId});

  /// Issue a JSON-RPC-lite request and await its result payload.
  Future<Map<String, dynamic>> request(
    String method, {
    Map<String, dynamic> params,
  });

  // ── Capture (Capture feed) ──

  /// Persist a new capture. Returns the freshly-created [Idea] (state
  /// `pending`); ingestion advances the state asynchronously server-side.
  ///
  /// [idempotencyKey] (when supplied) is sent on the wire as the
  /// `Idempotency-Key` header so the server dedups a retried capture
  /// (immediate POST + a later flush of the same queued row share one key).
  ///
  /// [lat]/[lng]/[accuracyM] (LOC-R1) carry an optional action-time location
  /// the caller (the funnel, Task 14c) supplies; when null the capture is
  /// byte-identical to before. They are stamped on the queued [Idea], sent in
  /// the POST body, and re-sent on flush so a queued capture keeps its location.
  ///
  /// [capturedAt] (Part F / LOC-R3) is the PRECISE action-time instant. The
  /// funnel stamps it ONCE at enqueue; the immediate POST and any later offline
  /// flush re-send THAT instant, so a capture made at 08:30 and flushed at noon
  /// is stamped 08:30 — not noon — server-side (the backend `COALESCE`s a client
  /// `captured_at` over `now()`). When null the server falls back to its receive
  /// time, byte-identical to before.
  Future<Idea> capture({
    required String body,
    required String origin,
    String? idempotencyKey,
    double? lat,
    double? lng,
    double? accuracyM,
    DateTime? capturedAt,
  });

  /// The reverse-chronological capture feed (newest first). With [day] set, returns ALL captures from
  /// that LOCAL calendar day (the calendar/date-picker view) instead of the default newest-N feed.
  Future<List<Idea>> captures({DateTime? day});

  /// One capture's full detail (CAP-R1); `null` if not found / deleted.
  Future<Idea?> captureDetail(String id);

  /// Edit a capture (CAP-R2). A body change re-distills server-side; an unchanged body is a no-op
  /// for the wiki. Location fields update independently.
  Future<void> updateCapture(String id, {String? body, double? lat, double? lng, double? locAccuracyM});

  /// Delete a capture (CAP-R3): soft-delete + wiki cascade server-side, undoable from Activity.
  Future<void> deleteCapture(String id);

  /// Cloud STT (D4): transcribe a recorded audio clip server-side with the tenant's selected (or
  /// auto-derived) BYOK speech-to-text provider (OpenAI/Groq/Zhipu/Gemini/Qwen/MiniMax), then the app
  /// drops the text into the editable compose field for review before capture. Return-envelope: it resolves with the
  /// transcript (possibly empty on a provider failure) and never echoes the raw audio.
  /// [audio] is the recorded clip bytes (base64 is the wire form); [language] is an
  /// optional ISO-639-1 hint (null = auto-detect).
  Future<String> transcribe({required List<int> audio, String? language});

  // ── Wiki (Wiki browser) ──

  /// The wiki index: every page with title + one-line summary (no body).
  Future<List<WikiPage>> wikiIndex();

  /// A single wiki page by slug, with body + backlinks; `null` if not found.
  Future<WikiPage?> wikiPage(String slug);

  // ── Recall (Recall chat) ──

  /// Ask a question; returns the streamed SSE taxonomy token-by-token. When
  /// [threadId] is supplied (continue), the server reuses that thread id and the
  /// model sees the prior turns (REC-R4/REC-R7); otherwise a fresh thread is minted.
  ///
  /// v0.2.2: an optional per-recall override — [provider]/[model] pick a BYOK
  /// model and [effort] (a `RecallEffort.wire` token) sets reasoning effort. All
  /// null = the server/tenant default. They travel as `?provider=&model=&effort=`
  /// query params; the backend resolves them within the tenant's configured keys.
  Stream<SseEvent> recallStream(
    String question, {
    String? threadId,
    String? provider,
    String? model,
    String? effort,
  });

  /// The recall conversation history (newest first) for the left drawer (REC-R8).
  Future<List<ConversationSummary>> listConversations();

  /// The prior turns of one conversation, in order, for a reopened thread (REC-R7).
  Future<List<ConversationMessage>> getConversationMessages(String threadId);

  // ── Activity (ingest review · Dream detail · jobs + cost) ──

  /// The pending ingest-review candidates (`status='pending'`, the D13 gate):
  /// `wiki_merge` / `wiki_delete` proposed edits awaiting Approve/Reject.
  Future<List<Approval>> approvals();

  /// Approve (`approve=true`) or reject a candidate. The app NEVER canonicalizes
  /// itself — it only proposes the decision; the server applies it (D13).
  Future<void> respondApproval(String id, bool approve);

  /// DIG-R6 — run the one-click incremental digest (`POST /v1/ingest/run`): enqueue an ingest job for
  /// each new/edited capture. Returns the number of jobs enqueued (distinct dirty captures; repeated
  /// calls debounce onto existing pending jobs server-side). Jobs run only when the server's ingest
  /// worker is enabled; progress is observed via the jobs list (`jobs()`).
  Future<int> runIngest();

  /// Recent jobs (ingest/lint/dream/transcribe) with their `job_state`, newest
  /// first — drives the StatusDot-mapped job list.
  Future<List<JobRow>> jobs();

  /// Today's spend in micros (server-aggregated; the UI never sums usage).
  Future<int> todayCostMicros();

  /// The live Dream-progress stream for a running dream job (Appendix A §4.1):
  /// `dream_started → progress* → completed|failed`.
  Stream<SseEvent> dreamEvents(String jobId);

  /// Cancel a running job (the Dream Cancel control → `DreamTask.kill` + clock
  /// rollback server-side, Appendix A A-R8).
  Future<void> cancelJob(String jobId);

  // ── Settings (BYOK vault · model picker · cost ledger · privacy) ──

  /// The configured BYOK providers — each a [ProviderCredential] carrying only a
  /// masked `key_hint` + health `status` (the secret never leaves the vault).
  Future<List<ProviderCredential>> credentials();

  /// Store/replace a provider key. The plaintext goes server-side (vault);
  /// the returned credential carries ONLY the masked `key_hint` (security
  /// boundary — the secret is never returned or displayed after entry).
  Future<ProviderCredential> putKey(String provider, String key);

  /// Remove a provider's key from the vault.
  Future<void> deleteKey(String provider);

  /// Permanently delete the caller's account (Apple Guideline 5.1.1(v)). The
  /// backend purges the tenant and ALL synced data + keys; the app then tears
  /// down local tokens/session/cache and returns to /login.
  Future<void> deleteAccount();

  /// The selectable models for a provider (the `fetch_models` surface).
  Future<List<String>> fetchModels(String provider);

  /// The active model chosen for a provider, or null if none picked.
  Future<String?> activeModel(String provider);

  /// Choose the active model for a provider.
  Future<void> setActiveModel(String provider, String model);

  /// Per-day/scope cost rows (the 5 token kinds + cost in micros), newest first.
  Future<List<CostLedgerRow>> costLedger();

  /// Whether server-side nightly Auto-Dream is enabled (the D9 posture).
  Future<bool> serverDream();

  /// Toggle the server-readable / server-Dream posture (D9).
  Future<void> setServerDream(bool on);

  // ── Voice transcription provider (D4 multi-provider STT) ──

  /// The STT picker payload (`GET /v1/transcribe/providers`): configured STT-capable providers,
  /// all STT-capable vendors, and the current explicit selection (null ⇒ Auto / auto-derive).
  Future<SttProviders> sttProviders();

  /// Set the explicit STT provider (`PUT /v1/settings/stt-provider`). `null` (or "auto") ⇒ Auto:
  /// the server auto-derives the provider from the configured BYOK keys.
  Future<void> setSttProvider(String? provider);

  // ── Sync (Phase 1: read sync) ──

  /// Register this device with the tenant (`POST /v1/sync/register`). Returns the
  /// per-tenant [DeviceReg] (device_id + HLC site_id, ≥1). Idempotent server-side.
  Future<DeviceReg> registerDevice(String platform);

  /// Pull the change feed (`GET /v1/sync/pull?since=<seq>`). A cold pull
  /// ([since] == 0) returns a snapshot bootstrap (SYNC-D5); a warm pull returns
  /// incremental ops by `seq` (SYNC-D4). Either way the [SyncDelta.cursor] is the
  /// new watermark the client persists.
  Future<SyncDelta> pullSync({required int since});

  // ── App update (release manifest) ──

  /// AU-R15 — fetch the release manifest for [platform] ("android"|"ios"). Unauthenticated metadata
  /// (the server requires no JWT here); used to drive the update nudge + force-gate.
  Future<AppReleaseManifest> fetchReleaseManifest(String platform);

  /// Tear down the connection and release resources.
  Future<void> dispose();
}

/// A seedable in-memory client so the app — and especially the 3 content
/// screens — runs against realistic content before the real transport lands.
/// `const StubApiClient()` is the inert (empty) boot stub; `StubApiClient.
/// seeded()` carries fixtures. Replaced wholesale next milestone.
class StubApiClient implements QcueApiClient {
  StubApiClient._(
    this._ideas,
    this._pages, {
    List<Approval>? approvals,
    List<JobRow>? jobs,
    List<ProviderCredential>? credentials,
    List<CostLedgerRow>? costLedger,
    Map<String, List<String>>? models,
    Map<String, String>? activeModels,
    List<ConversationSummary>? conversations,
    Map<String, List<ConversationMessage>>? messages_,
    this._todayCostMicros = 0,
    this.serverDreamEnabled = true,
  })  : _approvals = approvals ?? [],
        _jobs = jobs ?? const [],
        _credentials = credentials ?? [],
        _costLedger = costLedger ?? const [],
        _models = models ?? const {},
        _activeModels = activeModels ?? {},
        _conversations = conversations ?? const [],
        _messages = messages_ ?? const {};

  /// Inert stub: every read is empty, every stream yields offline/empty states.
  factory StubApiClient() => StubApiClient._([], const {});

  /// Realistic fixtures for the content screens.
  factory StubApiClient.seeded() {
    final now = DateTime.utc(2026, 6, 13, 9, 30);
    Idea idea(
      String id,
      String body,
      IngestState state,
      Duration ago, {
      IdeaKind kind = IdeaKind.text,
      String origin = 'capture',
    }) =>
        Idea(
          id: id,
          tenantId: 't-1',
          userId: 'u-1',
          kind: kind,
          body: body,
          origin: origin,
          ingestState: state,
          capturedAt: now.subtract(ago),
        );

    // Reverse-chronological seed spanning every ingest_state (status dots).
    final ideas = <Idea>[
      idea('i-6', 'Try the gate-ladder idea on the Dream scheduler.',
          IngestState.ingesting, const Duration(minutes: 3)),
      idea('i-5', 'Embeddings vs. grep recall — revisit the trade-off.',
          IngestState.ingested, const Duration(minutes: 41)),
      idea('i-4', 'Note to self: cite the source line, not the whole file.',
          IngestState.ingested, const Duration(hours: 2)),
      idea('i-3', 'Same thought as yesterday about CRDT sync.',
          IngestState.skippedRedundant, const Duration(hours: 5)),
      idea('i-2', 'Voice memo about the recall SSE taxonomy.',
          IngestState.pending, const Duration(days: 1, hours: 1),
          kind: IdeaKind.voice, origin: 'voice'),
      idea('i-1', 'Clipped: WCAG contrast for link text needs 4.5:1.',
          IngestState.failed, const Duration(days: 1, hours: 4),
          kind: IdeaKind.clip, origin: 'web'),
    ];

    final pages = <String, WikiPage>{
      for (final p in _seedPages(now)) p.slug: p,
    };

    // ── Activity: pending candidates (D13 gate), jobs, today's cost ──
    final approvals = <Approval>[
      const Approval(
        id: 'ap-1',
        action: 'wiki_merge',
        status: ApprovalStatus.pending,
        requestedBy: 'dream',
        subjectRef: {
          'target_slug': 'recall-architecture',
          'summary':
              'Merge "Grep recall" into Recall Architecture (dedupe overlap).',
        },
      ),
      const Approval(
        id: 'ap-2',
        action: 'wiki_delete',
        status: ApprovalStatus.pending,
        requestedBy: 'dream',
        subjectRef: {
          'target_slug': 'stale-note',
          'summary': 'Delete the orphaned "Stale note" page (no backlinks).',
        },
      ),
    ];

    final jobs = <JobRow>[
      const JobRow(
          id: 'd-running',
          kind: JobKind.dream,
          state: JobState.leased,
          progress: 0.6),
      const JobRow(id: 'j-ingest-1', kind: JobKind.ingest, state: JobState.done),
      const JobRow(
          id: 'j-lint-1', kind: JobKind.lint, state: JobState.pending),
      const JobRow(
          id: 'j-tr-1', kind: JobKind.transcribe, state: JobState.done),
      const JobRow(
          id: 'j-ingest-2',
          kind: JobKind.ingest,
          state: JobState.failed,
          lastError: 'provider rate limit'),
      const JobRow(
          id: 'd-done', kind: JobKind.dream, state: JobState.done),
    ];

    // ── Settings: masked BYOK vault, models, cost ledger, privacy ──
    final credentials = <ProviderCredential>[
      const ProviderCredential(
          provider: 'openai', keyHint: 'sk-…AB12', status: CredStatus.ok),
      const ProviderCredential(
          provider: 'anthropic', keyHint: 'sk-…9F3D', status: CredStatus.ok),
      const ProviderCredential(
          provider: 'gemini', keyHint: 'AI…ZK7', status: CredStatus.exhausted),
      const ProviderCredential(
          provider: 'deepseek', keyHint: 'sk-…00XY', status: CredStatus.dead),
    ];

    // Mirrors the backend's curated catalog (dispatch::provider_models): newest flagship + one low-price.
    final models = <String, List<String>>{
      'openai': ['gpt-5.5', 'gpt-5.4-mini'],
      'anthropic': ['claude-opus-4-8', 'claude-haiku-4-5'],
      'gemini': ['gemini-3-pro', 'gemini-3-flash'],
      'deepseek': ['deepseek-v4-pro', 'deepseek-v4-flash'],
    };
    final activeModels = <String, String>{
      'openai': 'gpt-5.5',
      'anthropic': 'claude-opus-4-8',
    };

    final costLedger = <CostLedgerRow>[
      CostLedgerRow(
          day: DateTime.utc(2026, 6, 13),
          inputTokens: 12400,
          outputTokens: 3210,
          cacheReadTokens: 8100,
          cacheWriteTokens: 1200,
          reasoningTokens: 640,
          costMicros: 420000),
      CostLedgerRow(
          day: DateTime.utc(2026, 6, 12),
          inputTokens: 31002,
          outputTokens: 8114,
          cacheReadTokens: 15400,
          cacheWriteTokens: 2200,
          reasoningTokens: 1980,
          costMicros: 1070000),
      CostLedgerRow(
          day: DateTime.utc(2026, 6, 11),
          inputTokens: 9800,
          outputTokens: 2400,
          cacheReadTokens: 4100,
          cacheWriteTokens: 800,
          reasoningTokens: 310,
          costMicros: 305000),
    ];

    final conversations = <ConversationSummary>[
      const ConversationSummary(
        id: 'th-seed-1',
        title: 'What did I decide about embeddings?',
        updatedAt: '2026-06-13T09:00:00Z',
        lastSnippet: 'You chose grep recall.',
      ),
    ];
    final conversationMessages = <String, List<ConversationMessage>>{
      'th-seed-1': const [
        ConversationMessage(role: 'user', content: 'What did I decide about embeddings?'),
        ConversationMessage(role: 'assistant', content: 'You chose grep recall over vectors.'),
      ],
    };

    return StubApiClient._(
      ideas,
      pages,
      approvals: approvals,
      jobs: jobs,
      credentials: credentials,
      costLedger: costLedger,
      models: models,
      activeModels: activeModels,
      conversations: conversations,
      messages_: conversationMessages,
      todayCostMicros: 420000,
      serverDreamEnabled: true,
    );
  }

  final List<Idea> _ideas;
  final Map<String, WikiPage> _pages;
  final List<Approval> _approvals;
  final List<JobRow> _jobs;
  final List<ProviderCredential> _credentials;
  final List<CostLedgerRow> _costLedger;
  final Map<String, List<String>> _models;
  final Map<String, String> _activeModels;
  final List<ConversationSummary> _conversations;
  final Map<String, List<ConversationMessage>> _messages;
  final int _todayCostMicros;

  /// The D9 server-readable / server-Dream posture (mutable in the stub).
  bool serverDreamEnabled;
  int _nextId = 100;

  @override
  Stream<ApiConnectionState> get connectionState =>
      Stream<ApiConnectionState>.value(ApiConnectionState.disconnected);

  @override
  Stream<RuntimeEventEnvelope> events({required String threadId}) =>
      const Stream<RuntimeEventEnvelope>.empty();

  @override
  Future<Map<String, dynamic>> request(
    String method, {
    Map<String, dynamic> params = const {},
  }) async =>
      const {};

  @override
  Future<Idea> capture({
    required String body,
    required String origin,
    String? idempotencyKey,
    double? lat,
    double? lng,
    double? accuracyM,
    DateTime? capturedAt,
  }) async {
    final idea = Idea(
      id: 'i-stub-${_nextId++}',
      tenantId: 't-1',
      userId: 'u-1',
      kind: IdeaKind.text,
      body: body,
      origin: origin,
      ingestState: IngestState.pending,
      capturedAt: capturedAt ?? DateTime.now().toUtc(),
      lat: lat,
      lng: lng,
      locAccuracyM: accuracyM,
    );
    _ideas.insert(0, idea); // newest first
    return idea;
  }

  @override
  Future<String> transcribe({required List<int> audio, String? language}) async =>
      // Canned cloud transcript so the voice path is exercisable under the stub
      // without a real provider call.
      'Cloud transcript: revisit the recall trade-off.';

  @override
  Future<List<Idea>> captures({DateTime? day}) async => day == null
      ? List.unmodifiable(_ideas)
      : List.unmodifiable(_ideas.where((i) => sameLocalDay(i.capturedAt, day)));

  @override
  Future<Idea?> captureDetail(String id) async {
    for (final i in _ideas) {
      if (i.id == id) return i;
    }
    return null;
  }

  @override
  Future<void> updateCapture(String id, {String? body, double? lat, double? lng, double? locAccuracyM}) async {
    final ix = _ideas.indexWhere((i) => i.id == id);
    if (ix < 0) return;
    _ideas[ix] = _ideas[ix].copyWith(body: body, lat: lat, lng: lng, locAccuracyM: locAccuracyM);
  }

  @override
  Future<void> deleteCapture(String id) async => _ideas.removeWhere((i) => i.id == id);

  @override
  Future<List<WikiPage>> wikiIndex() async => List.unmodifiable(
        _pages.values.map((p) => p.copyWithoutBody()),
      );

  @override
  Future<WikiPage?> wikiPage(String slug) async => _pages[slug];

  @override
  Stream<SseEvent> recallStream(
    String question, {
    String? threadId,
    String? provider,
    String? model,
    String? effort,
  }) async* {
    // The stub ignores the override (no real provider) — the live path applies it.
    yield SessionStarted(threadId ?? 'th-stub');
    yield const ToolCall('recall_search');
    yield const ToolResult('recall_search');
    yield const ReasoningDelta(
        'The user asked about embeddings. I recall a captured note weighing ');
    yield const ReasoningDelta('grep recall against vector search.');
    // Streamed token-by-token, with an inline [[wikilink]] for navigation.
    const tokens = [
      'You decided ',
      'against embeddings: ',
      'plain grep recall ',
      'kept the system simpler. ',
      'See [[Recall Architecture]] ',
      'for the trade-off.',
    ];
    for (final t in tokens) {
      yield MessageDelta(t);
    }
    yield const CitationEvent(
      Citation(relPath: 'recall-architecture.md', startLine: 42, endLine: 47),
    );
    yield const UsageEvent(
        inputTokens: 1280, outputTokens: 64, reasoningTokens: 22);
    yield const DoneEvent();
  }

  // ── Activity ──

  @override
  Future<List<Approval>> approvals() async => List.unmodifiable(_approvals);

  @override
  Future<void> respondApproval(String id, bool approve) async {
    _approvals.removeWhere((a) => a.id == id);
  }

  @override
  Future<int> runIngest() async {
    // Keyless stub: pretend the pending captures in the seed are dirty.
    return _ideas.where((i) => i.ingestState == IngestState.pending).length;
  }

  @override
  Future<List<ConversationSummary>> listConversations() async =>
      List.unmodifiable(_conversations);

  @override
  Future<List<ConversationMessage>> getConversationMessages(String threadId) async =>
      List.unmodifiable(_messages[threadId] ?? const []);

  @override
  Future<List<JobRow>> jobs() async => List.unmodifiable(_jobs);

  @override
  Future<int> todayCostMicros() async => _todayCostMicros;

  @override
  Stream<SseEvent> dreamEvents(String jobId) async* {
    yield const DreamStarted(jobId: 'd-running', sessions: 6);
    // 8 turns (>VISIBLE_TURNS=6) so the collapse discipline is exercised, with a
    // dim per-turn tool count and a growing set of touched pages.
    const phases = ['Orient', 'Gather', 'Consolidate', 'Prune'];
    final touched = <String>{};
    for (var i = 0; i < 8; i++) {
      final phase = phases[(i ~/ 2).clamp(0, phases.length - 1)];
      if (i >= 3) touched.add('concepts/auto-dream.md');
      if (i >= 5) touched.add('index.md');
      if (i >= 6) touched.add('concepts/recall-architecture.md');
      yield DreamProgress(
        text: '$phase — reviewed signal in turn $i',
        toolUseCount: i,
        pagesTouched: touched.toList(),
      );
    }
    yield const DreamCompleted(pagesImproved: 3);
  }

  @override
  Future<void> cancelJob(String jobId) async {
    final i = _jobs.indexWhere((j) => j.id == jobId);
    if (i >= 0) {
      final j = _jobs[i];
      _jobs[i] = JobRow(id: j.id, kind: j.kind, state: JobState.canceled);
    }
  }

  // ── Settings ──

  @override
  Future<List<ProviderCredential>> credentials() async =>
      List.unmodifiable(_credentials);

  @override
  Future<ProviderCredential> putKey(String provider, String key) async {
    final cred = ProviderCredential(
      provider: provider,
      keyHint: _maskKey(key), // ONLY the masked hint ever surfaces
      status: CredStatus.ok,
    );
    _credentials
      ..removeWhere((c) => c.provider == provider)
      ..add(cred);
    return cred;
  }

  @override
  Future<void> deleteKey(String provider) async {
    _credentials.removeWhere((c) => c.provider == provider);
  }

  @override
  Future<void> deleteAccount() async {
    // Account deleted server-side → drop every in-memory trace. These four
    // collections are always growable in both constructors, so clear() is safe.
    _ideas.clear();
    _credentials.clear();
    _approvals.clear();
    _activeModels.clear();
  }

  @override
  Future<List<String>> fetchModels(String provider) async =>
      _models[provider] ?? const [];

  @override
  Future<String?> activeModel(String provider) async => _activeModels[provider];

  @override
  Future<void> setActiveModel(String provider, String model) async {
    _activeModels[provider] = model;
  }

  @override
  Future<List<CostLedgerRow>> costLedger() async =>
      List.unmodifiable(_costLedger);

  @override
  Future<bool> serverDream() async => serverDreamEnabled;

  @override
  Future<void> setServerDream(bool on) async {
    serverDreamEnabled = on;
  }

  /// In-memory STT provider choice so the stub roundtrips set→get (null ⇒ Auto).
  String? sttProviderChoice;

  @override
  Future<SttProviders> sttProviders() async => SttProviders(
        selected: sttProviderChoice,
        available: const ['openai', 'zhipu'],
        allCapable: const ['openai', 'groq', 'zhipu', 'gemini', 'qwen', 'minimax'],
      );

  @override
  Future<void> setSttProvider(String? provider) async {
    sttProviderChoice = (provider == null || provider == 'auto') ? null : provider;
  }

  // ── Sync (Phase 1) ──

  @override
  Future<DeviceReg> registerDevice(String platform) async =>
      // A deterministic stub device so the keyless demo + tests have sync.
      const DeviceReg(deviceId: 'stub-device', siteId: 1);

  @override
  Future<SyncDelta> pullSync({required int since}) async {
    // The seeded stub has a fixed corpus, so its whole change feed is the
    // snapshot watermark: a cold pull (since:0) returns the snapshot built from
    // the seeds; a warm pull (since:cursor) has no new ops. Cursor == seed count.
    final cursor = _ideas.length + _pages.length;
    if (since > 0) {
      return SyncDelta(cursor: cursor);
    }
    return SyncDelta(
      cursor: cursor,
      snapshot: SyncSnapshot(
        ideas: [
          for (final i in _ideas)
            IdeaSnap(
              id: i.id,
              body: i.body,
              origin: i.origin,
              capturedAt: i.capturedAt.toUtc().toIso8601String(),
            ),
        ],
        wikiPages: [
          for (final p in _pages.values)
            WikiPageSnap(
              slug: p.slug,
              title: p.title,
              contentHash: _stubContentHash(p.bodyMarkdown),
              syncVersion: 1,
            ),
        ],
      ),
    );
  }

  /// A deterministic, non-cryptographic content fingerprint for the seeded
  /// snapshot (the stub has no sha256 dependency; the real backend sends the true
  /// content_hash). FNV-1a over the body, hex-encoded — stable across runs.
  static String _stubContentHash(String body) {
    var hash = 0x811c9dc5;
    for (final unit in body.codeUnits) {
      hash ^= unit;
      hash = (hash * 0x01000193) & 0xffffffff;
    }
    return hash.toRadixString(16).padLeft(8, '0');
  }

  @override
  Future<AppReleaseManifest> fetchReleaseManifest(String platform) async =>
      // Keyless stub: pretend we are always on the latest build (no nudge, no force-gate).
      AppReleaseManifest.none;

  @override
  Future<void> dispose() async {}

  /// Mask a secret to the canonical `prefix…last4` hint (the only thing the
  /// vault UI is ever allowed to display). Never returns the secret.
  static String _maskKey(String key) {
    final k = key.trim();
    final last4 = k.length >= 4 ? k.substring(k.length - 4) : k;
    final prefix = k.length >= 3 ? k.substring(0, 3) : '';
    return prefix.isEmpty ? '…$last4' : '$prefix…$last4';
  }

  static List<WikiPage> _seedPages(DateTime now) {
    return [
      WikiPage(
        id: 'w-1',
        type: WikiPageType.concept,
        slug: 'auto-dream',
        title: 'Auto-Dream',
        summary:
            'The nightly consolidation pass that improves wiki pages offline.',
        bodyMarkdown: '## Auto-Dream\n\n'
            'Auto-Dream is the scheduled consolidation agent. It runs a '
            'gate-ladder over [[Recall Architecture]] and the capture log, '
            'then proposes edits behind [[Approvals]].\n\n'
            '- Reads the wiki read-only, like a fork.\n'
            '- Emits an "Improved N pages" report.\n',
        updated: now.subtract(const Duration(hours: 6)),
        tags: const ['dream', 'agent'],
        backlinks: const [
          WikiLink(
              targetSlug: 'recall-architecture',
              targetPageId: 'w-2',
              display: 'Recall Architecture'),
          WikiLink(
              targetSlug: 'index', targetPageId: 'w-5', display: 'Index'),
        ],
      ),
      WikiPage(
        id: 'w-2',
        type: WikiPageType.concept,
        slug: 'recall-architecture',
        title: 'Recall Architecture',
        summary: 'How recall answers questions over your captures and wiki.',
        bodyMarkdown: '## Recall Architecture\n\n'
            'Recall uses grep-style retrieval rather than embeddings. The '
            'agent cites the exact source line. Compare with [[Auto-Dream]], '
            'which consolidates the same corpus overnight.\n\n'
            'See also the [[Missing Page]] that has not been written yet.\n',
        updated: now.subtract(const Duration(days: 2)),
        tags: const ['recall', 'architecture'],
        backlinks: const [
          WikiLink(
              targetSlug: 'auto-dream',
              targetPageId: 'w-1',
              display: 'Auto-Dream'),
        ],
      ),
      WikiPage(
        id: 'w-3',
        type: WikiPageType.entity,
        slug: 'approvals',
        title: 'Approvals',
        summary: 'The human-in-the-loop gate for destructive wiki edits.',
        bodyMarkdown: '## Approvals\n\n'
            'Destructive edits (merge, delete) from [[Auto-Dream]] wait here '
            'for confirmation.\n',
        updated: now.subtract(const Duration(days: 3)),
        tags: const ['safety'],
        backlinks: const [
          WikiLink(
              targetSlug: 'auto-dream',
              targetPageId: 'w-1',
              display: 'Auto-Dream'),
        ],
      ),
      WikiPage(
        id: 'w-4',
        type: WikiPageType.source,
        slug: 'wcag-contrast',
        title: 'WCAG Contrast Notes',
        summary: 'Why link text needs a 4.5:1 ratio against the page.',
        bodyMarkdown: '## WCAG Contrast Notes\n\n'
            'Normal text must clear 4.5:1. Link text carries reading weight, '
            'so the link color itself must pass — not just the underline.\n',
        updated: now.subtract(const Duration(days: 4)),
        tags: const ['accessibility'],
        backlinks: const [],
      ),
      WikiPage(
        id: 'w-5',
        type: WikiPageType.indexPage,
        slug: 'index',
        title: 'Index',
        summary: 'Entry point to the knowledge base.',
        bodyMarkdown: '## Index\n\n'
            'Start at [[Recall Architecture]] or [[Auto-Dream]].\n',
        updated: now.subtract(const Duration(hours: 12)),
        tags: const [],
        backlinks: const [],
      ),
      WikiPage(
        id: 'w-6',
        type: WikiPageType.log,
        slug: 'daily-log-2026-06-13',
        title: 'Daily Log — 2026-06-13',
        summary: "Today's captures, consolidated.",
        bodyMarkdown: '## 2026-06-13\n\n'
            'Revisited [[Recall Architecture]]; queued a Dream run.\n',
        updated: now,
        tags: const ['log'],
        backlinks: const [],
      ),
    ];
  }
}
