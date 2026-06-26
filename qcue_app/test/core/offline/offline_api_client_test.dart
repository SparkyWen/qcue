// QCue S4-R25/R26/R56: the OfflineAwareApiClient decorates the real client with
// the IdeaCache. Canonical guarantees pinned here:
//   - capture() persists locally BEFORE the network — an offline capture is
//     never lost; it returns a queued/pending Idea and lands in the cached feed;
//   - reads (captures/wikiPage/wikiIndex) serve from cache when the network
//     throws, and refresh the cache on a successful network read;
//   - flushOutbox() POSTs the queued captures idempotently and reconciles state;
//   - everything else delegates to the wrapped client unchanged.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/models/recall_conversation.dart';
import 'package:qcue_app/core/models/runtime_event.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/models/app_release_manifest.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';
import 'package:qcue_app/core/offline/offline_api_client.dart';
import 'package:qcue_app/core/offline/sync_status.dart';
import 'package:qcue_app/core/sync/sync_dtos.dart';

/// A fake inner client whose network can be flipped offline (the cache-relevant
/// calls throw) and which records the capture bodies the server "received". It
/// composes a seeded [StubApiClient] (a factory, so we delegate rather than
/// extend) and only intercepts the four methods the decorator caches.
class FakeInner implements QcueApiClient {
  final StubApiClient _stub = StubApiClient.seeded();
  bool online = true;
  final List<String> captured = [];

  /// The Idempotency-Key the decorator passed on each capture POST (Task 6).
  final List<String?> idempotencyKeys = [];

  /// The action-time `captured_at` the decorator passed on each capture POST
  /// (Part F / LOC-R3) — so a flush can be asserted to carry the ORIGINAL time.
  final List<DateTime?> capturedAts = [];

  void _guard() {
    if (!online) throw Exception('network down');
  }

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
    _guard();
    captured.add(body);
    idempotencyKeys.add(idempotencyKey);
    capturedAts.add(capturedAt);
    return _stub.capture(
        body: body,
        origin: origin,
        lat: lat,
        lng: lng,
        accuracyM: accuracyM,
        capturedAt: capturedAt);
  }

  @override
  Future<List<Idea>> captures({DateTime? day}) async {
    _guard();
    return _stub.captures();
  }

  @override
  Future<Idea?> captureDetail(String id) => _stub.captureDetail(id);
  @override
  Future<void> updateCapture(String id, {String? body, double? lat, double? lng, double? locAccuracyM}) =>
      _stub.updateCapture(id, body: body, lat: lat, lng: lng, locAccuracyM: locAccuracyM);
  @override
  Future<void> deleteCapture(String id) => _stub.deleteCapture(id);

  @override
  Future<List<WikiPage>> wikiIndex() async {
    _guard();
    return _stub.wikiIndex();
  }

  @override
  Future<WikiPage?> wikiPage(String slug) async {
    _guard();
    return _stub.wikiPage(slug);
  }

  /// When set, transcribe() throws this (simulates a provider/no-key failure that the server
  /// reports as {success:false} — a real failure that must reach the UI, not be swallowed).
  Object? transcribeError;

  @override
  Future<String> transcribe({required List<int> audio, String? language}) async {
    if (transcribeError != null) throw transcribeError!;
    _guard();
    return _stub.transcribe(audio: audio, language: language);
  }

  // everything else delegates straight to the seeded stub
  @override
  Stream<ApiConnectionState> get connectionState => _stub.connectionState;
  @override
  Stream<RuntimeEventEnvelope> events({required String threadId}) =>
      _stub.events(threadId: threadId);
  @override
  Future<Map<String, dynamic>> request(String method,
          {Map<String, dynamic> params = const {}}) =>
      _stub.request(method, params: params);
  @override
  Stream<SseEvent> recallStream(String question,
          {String? threadId, String? provider, String? model, String? effort}) =>
      _stub.recallStream(question,
          threadId: threadId, provider: provider, model: model, effort: effort);
  @override
  Future<List<ConversationSummary>> listConversations() => _stub.listConversations();
  @override
  Future<List<ConversationMessage>> getConversationMessages(String threadId) =>
      _stub.getConversationMessages(threadId);
  @override
  Future<List<Approval>> approvals() => _stub.approvals();
  @override
  Future<void> respondApproval(String id, bool approve) =>
      _stub.respondApproval(id, approve);
  @override
  Future<int> runIngest() => _stub.runIngest();
  @override
  Future<List<JobRow>> jobs() => _stub.jobs();
  @override
  Future<int> todayCostMicros() => _stub.todayCostMicros();
  @override
  Stream<SseEvent> dreamEvents(String jobId) => _stub.dreamEvents(jobId);
  @override
  Future<void> cancelJob(String jobId) => _stub.cancelJob(jobId);
  @override
  Future<List<ProviderCredential>> credentials() => _stub.credentials();
  @override
  Future<ProviderCredential> putKey(String provider, String key) =>
      _stub.putKey(provider, key);
  @override
  Future<void> deleteKey(String provider) => _stub.deleteKey(provider);

  bool deletedAccount = false;
  @override
  Future<void> deleteAccount() async {
    deletedAccount = true;
    await _stub.deleteAccount();
  }
  @override
  Future<List<String>> fetchModels(String provider) =>
      _stub.fetchModels(provider);
  @override
  Future<String?> activeModel(String provider) => _stub.activeModel(provider);
  @override
  Future<void> setActiveModel(String provider, String model) =>
      _stub.setActiveModel(provider, model);
  @override
  Future<List<CostLedgerRow>> costLedger() => _stub.costLedger();
  @override
  Future<bool> serverDream() => _stub.serverDream();
  @override
  Future<void> setServerDream(bool on) => _stub.setServerDream(on);
  @override
  Future<DeviceReg> registerDevice(String platform) =>
      _stub.registerDevice(platform);
  @override
  Future<SyncDelta> pullSync({required int since}) =>
      _stub.pullSync(since: since);
  @override
  Future<AppReleaseManifest> fetchReleaseManifest(String platform) =>
      _stub.fetchReleaseManifest(platform);

  @override
  Future<void> dispose() => _stub.dispose();
}

/// A fake server that IGNORES the `day` window (and online state) and always returns a fixed row set —
/// it models a backend that doesn't honour `?start&end` (e.g. a stale deploy), so the decorator's own
/// day-scoping is what's under test.
class _UnfilteredServer extends FakeInner {
  _UnfilteredServer(this.rows);
  final List<Idea> rows;
  @override
  Future<List<Idea>> captures({DateTime? day}) async => rows;
}

OfflineAwareApiClient _client(FakeInner inner) =>
    OfflineAwareApiClient(inner, IdeaCache(InMemoryCacheStore(), feedCap: 100));

void main() {
  test('S4-R25: an offline capture is persisted+queued BEFORE the network',
      () async {
    final inner = FakeInner()..online = false;
    final api = _client(inner);

    final idea = await api.capture(body: 'offline thought', origin: 'capture');

    expect(inner.captured, isEmpty); // never reached the server
    expect(idea.ingestState, IngestState.pending); // queued/pending
    expect(api.cache.outbound().single.idea.id, idea.id);
    // the cached feed shows it immediately (offline read serves from cache)
    final feed = await api.captures();
    expect(feed.map((i) => i.id), contains(idea.id));
  });

  test('S4-R25: an online capture POSTs then caches', () async {
    final inner = FakeInner();
    final api = _client(inner);
    final idea = await api.capture(body: 'live thought', origin: 'capture');
    expect(inner.captured, contains('live thought')); // hit the server
    expect(api.cache.outbound(), isEmpty); // nothing queued
    expect(api.cache.feed().map((i) => i.id), contains(idea.id));
  });

  test('D4: transcribe propagates a TranscribeException (never swallows to empty)',
      () async {
    final inner = FakeInner()
      ..transcribeError = const TranscribeException('openai stt 401: bad key');
    final api = _client(inner);
    await expectLater(
      () => api.transcribe(audio: const [1]),
      throwsA(isA<TranscribeException>()),
    );
  });

  test('D4: transcribe maps a raw offline throw to a network-kind failure',
      () async {
    final inner = FakeInner()..online = false; // _guard() throws a plain Exception
    final api = _client(inner);
    await expectLater(
      () => api.transcribe(audio: const [1]),
      throwsA(isA<TranscribeException>()
          .having((e) => e.kind, 'kind', TranscribeErrorKind.network)),
    );
  });

  test('captures() serves from cache when the network throws', () async {
    final inner = FakeInner();
    final api = _client(inner);
    // prime the cache with a successful online read
    final live = await api.captures();
    expect(live, isNotEmpty);
    // now go offline — the read degrades to the cached feed
    inner.online = false;
    final cached = await api.captures();
    expect(cached.map((i) => i.id).toSet(), live.map((i) => i.id).toSet());
  });

  test('a date-scoped day view still shows a today-queued capture (offline-first)',
      () async {
    final inner = FakeInner()..online = false;
    final api = _client(inner);
    // a capture whose POST fails stays queued (capturedAt = now → today).
    final queued = await api.capture(body: 'queued today', origin: 'capture');
    expect(api.cache.outbound(), isNotEmpty);

    // back online: a successful day-scoped GET for today must NOT hide the queued capture
    // (the live feed and the offline day-view both keep it — the online day-view must too).
    inner.online = true;
    final dayView = await api.captures(day: DateTime.now());
    expect(dayView.any((i) => i.id == queued.id), isTrue,
        reason: 'a capture made today must stay visible in the day view, like the live feed');
  });

  test('the online day-view shows ONLY the selected day, even if the server returns other days',
      () async {
    // Reproduces the calendar bug: a server that ignores ?start&end (e.g. a stale-deployed backend)
    // returns captures from EVERY day. The decorator must still scope the day-view to the chosen day —
    // never blindly trust the server's windowing — or the day-view leaks every other day's captures.
    Idea at(String id, DateTime localNoon) => Idea(
          id: id,
          tenantId: 't',
          userId: 'u',
          kind: IdeaKind.text,
          body: 'body-$id',
          origin: 'capture',
          ingestState: IngestState.ingested,
          capturedAt: localNoon.toUtc(), // stored UTC; toLocal() lands back on its local day (tz-robust)
        );
    final dayA = at('a', DateTime(2026, 3, 10, 12));
    final dayB = at('b', DateTime(2026, 3, 11, 12));
    final inner = _UnfilteredServer([dayA, dayB]);
    final api = _client(inner);

    final view = await api.captures(day: DateTime(2026, 3, 10));
    expect(view.map((i) => i.id).toList(), ['a'],
        reason: 'the day view for Mar 10 must contain only Mar 10, not Mar 11 that the server wrongly returned');
  });

  test('wikiPage()/wikiIndex() serve from cache when offline', () async {
    final inner = FakeInner();
    final api = _client(inner);
    final page = await api.wikiPage('auto-dream'); // online → cached
    expect(page, isNotNull);
    final index = await api.wikiIndex(); // online → cached
    expect(index, isNotEmpty);

    inner.online = false;
    final cachedPage = await api.wikiPage('auto-dream');
    expect(cachedPage?.slug, 'auto-dream');
    final cachedIndex = await api.wikiIndex();
    expect(cachedIndex.map((p) => p.slug).toSet(),
        index.map((p) => p.slug).toSet());
  });

  test('S4-R26: flushOutbox POSTs queued captures + flips them ingested',
      () async {
    final inner = FakeInner()..online = false;
    final api = _client(inner);
    final queued = await api.capture(body: 'queued one', origin: 'capture');
    expect(api.cache.outbound(), isNotEmpty);

    // reconnect and flush
    inner.online = true;
    await api.flushOutbox();

    expect(inner.captured, contains('queued one')); // POSTed on reconnect
    expect(api.cache.outbound(), isEmpty); // dequeued
    final row = api.cache.feed().firstWhere((i) => i.id == queued.id);
    expect(row.ingestState, IngestState.ingested);
  });

  test('S4-R26: a double flushOutbox never double-POSTs (idempotent)',
      () async {
    final inner = FakeInner()..online = false;
    final api = _client(inner);
    await api.capture(body: 'once', origin: 'capture');
    inner.online = true;
    await api.flushOutbox();
    await api.flushOutbox(); // duplicate reconnect
    expect(inner.captured.where((b) => b == 'once'), hasLength(1));
  });

  test('Task 6: the immediate online capture POSTs with the queued row key',
      () async {
    final inner = FakeInner();
    final api = _client(inner);
    await api.capture(body: 'live', origin: 'capture');
    // The decorator enqueued (stamping a key) then POSTed with THAT key.
    expect(inner.idempotencyKeys, hasLength(1));
    expect(inner.idempotencyKeys.single, isNotNull);
    expect(inner.idempotencyKeys.single, isNotEmpty);
  });

  test('Task 6: flush passes the queued row idempotency key (not dropped)',
      () async {
    final inner = FakeInner()..online = false;
    final api = _client(inner);
    await api.capture(body: 'queued', origin: 'capture');
    final queuedKey = api.cache.outbound().single.idempotencyKey;

    inner.online = true;
    await api.flushOutbox();

    // The flush POST carried the SAME key stamped at enqueue time (so the server
    // dedups a row that the immediate path might also have POSTed).
    expect(inner.idempotencyKeys.single, queuedKey);
  });

  test('Task 7: an offline capture records the failure reason (not silent)',
      () async {
    SyncErrorReason? recorded = SyncErrorReason.other; // sentinel
    final inner = FakeInner()..online = false;
    final api = OfflineAwareApiClient(
      inner,
      IdeaCache(InMemoryCacheStore(), feedCap: 100),
      onSyncResult: (r) => recorded = r,
    );
    await api.capture(body: 'offline', origin: 'capture');
    // A raw transport throw (FakeInner threw a plain Exception) → network reason.
    expect(recorded, SyncErrorReason.network);
  });

  test('Task 7: a successful online capture clears the failure reason',
      () async {
    SyncErrorReason? recorded = SyncErrorReason.network;
    final inner = FakeInner();
    final api = OfflineAwareApiClient(
      inner,
      IdeaCache(InMemoryCacheStore(), feedCap: 100),
      onSyncResult: (r) => recorded = r,
    );
    await api.capture(body: 'live', origin: 'capture');
    expect(recorded, isNull); // cleared on a clean upload
  });

  test('non-cache methods delegate to the wrapped client', () async {
    final inner = FakeInner();
    final api = _client(inner);
    final jobs = await api.jobs();
    expect(jobs, isNotEmpty); // came straight from the seeded inner
    final cost = await api.todayCostMicros();
    expect(cost, greaterThan(0));
  });

  test('Part F/LOC-R3: a flushed offline capture keeps its ORIGINAL action time',
      () async {
    // The bug: on flush the decorator re-POSTed WITHOUT captured_at, so a capture
    // made offline at 08:30 and flushed at noon got stamped noon server-side. The
    // funnel must stamp the action-time ONCE (at enqueue) and the flush must re-send
    // THAT instant — never a fresh now().
    final inner = FakeInner()..online = false;
    final api = _client(inner);

    final queued = await api.capture(body: 'thought at 8:30', origin: 'capture');
    expect(inner.captured, isEmpty); // never reached the server (offline)
    // the action-time the funnel stamped on the queued row at enqueue time
    final stampedAt = api.cache.outbound().single.idea.capturedAt;
    expect(stampedAt, queued.capturedAt);

    // reconnect + flush — the POST must carry the SAME action time, not now()
    inner.online = true;
    await api.flushOutbox();

    expect(inner.captured, contains('thought at 8:30')); // POSTed on reconnect
    expect(inner.capturedAts.single, stampedAt,
        reason: 'the flush must re-send the ORIGINAL action time, not a fresh now()');
  });

  test('deleteAccount delegates to the inner client AND wipes the local cache',
      () async {
    final inner = FakeInner();
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 100);
    cache.enqueueCapture(body: 'local note', origin: 'capture');
    cache.writeSyncMeta(
        const SyncMeta(cursor: 5, deviceId: 'd', siteId: 1, lamport: 3));
    expect(cache.feed(), isNotEmpty);
    final api = OfflineAwareApiClient(inner, cache);

    await api.deleteAccount();

    expect(inner.deletedAccount, isTrue, reason: 'server-side delete called');
    expect(cache.feed(), isEmpty, reason: 'cached feed wiped');
    expect(cache.outbound(), isEmpty, reason: 'outbound queue wiped');
    expect(cache.syncMeta(), isNull, reason: 'sync bookkeeping wiped');
  });
}
