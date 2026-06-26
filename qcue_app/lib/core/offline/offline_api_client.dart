// QCue S4-R25/R26/R56 (master §10; D5/D6 local-first): the offline-aware client.
// It DECORATES the real [QcueApiClient] (an [HttpApiClient] in production) with
// an [IdeaCache], adding offline capture + a read-cache + an idempotent outbound
// flush — all behind the UNCHANGED [QcueApiClient] seam, so screens/providers/
// widgets are untouched (the layering test stays green).
//
// CONTRACT
//   capture(): persist LOCALLY (feed + queue) FIRST, then try the network.
//     - online  → POST, then cache the server idea (nothing queued);
//     - offline / POST throws → return the queued (pending, `queued`) idea; the
//       feed shows it with the distinct "queued / will sync" dot.
//   captures()/wikiPage()/wikiIndex(): try the network; on success refresh the
//     cache and return fresh data; on failure (or offline) serve from cache.
//   flushOutbox(): POST the queued captures in order, idempotent by client id,
//     and reconcile (dequeue + flip to `ingested`). Safe to call repeatedly
//     (on reconnect and on app resume) — a duplicate flush never double-POSTs.
//   everything else: delegates to the wrapped client unchanged.
import 'dart:async';

import '../location/location_service.dart';
import '../models/app_release_manifest.dart';
import '../models/protocol_models.dart';
import '../models/recall_conversation.dart';
import '../models/runtime_event.dart';
import '../models/sse_event.dart';
import '../net/capture_query.dart';
import '../net/qcue_api_client.dart';
import '../sync/sync_dtos.dart';
import 'idea_cache.dart';
import 'sync_status.dart';

/// The default funnel gate: location tagging is OFF unless the bootstrap injects a
/// real `locationEnabled` reading the device-local toggle. A top-level function
/// reference is const, so it can be a default for a constructor param.
bool _locationDisabled() => false;

class OfflineAwareApiClient implements QcueApiClient {
  OfflineAwareApiClient(
    this._inner,
    this.cache, {
    this.onSyncResult,
    this.locationService = const GeolocatorLocationService(),
    this.locationEnabled = _locationDisabled,
  });

  final QcueApiClient _inner;

  /// LOC-R1: the action-time location source. The funnel calls it ONLY when
  /// [locationEnabled] is true and the caller didn't already supply a location;
  /// any failure returns a null fix (never blocks the capture). Defaults to the
  /// real geolocator-backed service; tests inject a fake.
  final LocationService locationService;

  /// LOC-R2: whether to attach an action-time fix. Reads the device-local toggle
  /// (off by default). The bootstrap wires it to the Settings store; the default
  /// keeps every existing caller's behavior (no location) unchanged.
  final bool Function() locationEnabled;

  /// Exposed so the bootstrap can flush on reconnect/resume and so the capture
  /// feed can read the queued state. The screens never touch this — they go
  /// through the [QcueApiClient] seam.
  final IdeaCache cache;

  /// Task 7: a sink for the last upload outcome so the UI can tell "not signed
  /// in" from "wrong URL" from "offline". `null` ⇒ the error reason isn't
  /// surfaced (e.g. host tests that don't care). Kept Riverpod-free here so the
  /// decorator stays a plain class; the bootstrap wires it to [syncStatusProvider].
  final void Function(SyncErrorReason? reason)? onSyncResult;

  // ── Capture ────────────────────────────────────────────────────────────

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
    // Part F / LOC-R3: this funnel is the single action-time stamp point (it is
    // already the single location-fetch point). Compute the action-time instant
    // ONCE here and thread it to BOTH the durable queued row and the immediate
    // POST, so an offline capture flushed later carries the time it was MADE.
    // The caller may pass an explicit instant (e.g. a re-issued capture); else now.
    final at = capturedAt ?? DateTime.now().toUtc();
    // LOC-R1/R2: the action-time GPS funnel. When the device-local toggle is on
    // AND the caller didn't already supply a location (e.g. an offline flush
    // replay carries its own), fetch a single fix. A null fix (toggle off /
    // permission denied / timeout) NEVER blocks the capture — it proceeds with a
    // null location. geolocator reports 0.0 accuracy when unknown → treat as null.
    if (lat == null && lng == null && locationEnabled()) {
      final fix = await locationService.currentFix();
      lat = fix?.lat;
      lng = fix?.lng;
      accuracyM = (fix?.accuracyM == 0.0) ? null : fix?.accuracyM;
    }
    // S4-R25: persist locally BEFORE any network — never lose an idea, even if
    // the process dies between here and the POST (the row is in the durable
    // queue + feed with the distinct queued dot). The resolved location + the
    // action-time `at` ride along to BOTH the queued row and the immediate POST.
    final queued = cache.enqueueCapture(
      body: body,
      origin: origin,
      lat: lat,
      lng: lng,
      accuracyM: accuracyM,
      capturedAt: at,
    );
    // The idempotency key was stamped on the queued row at enqueue time; the
    // immediate POST reuses it so a later flush of the SAME row dedups server-side.
    final key = cache.idempotencyKeyFor(queued.id);
    try {
      // POST this capture and reconcile the provisional row with the server's
      // authoritative idea (real id; `pending` → ingestion advances it server-
      // side). The idempotency key guards against a duplicate from a later
      // flushOutbox (e.g. a concurrent reconnect).
      final server = await _inner.capture(
        body: body,
        origin: origin,
        idempotencyKey: key,
        lat: lat,
        lng: lng,
        accuracyM: accuracyM,
        capturedAt: at,
      );
      cache.reconcileQueued(queued.id, server);
      onSyncResult?.call(null); // uploaded cleanly → clear any prior error
      return server;
    } catch (e) {
      // Offline / server unreachable / unauthorized: stays queued (pending) and
      // already sits in the cached feed with the distinct queued dot — flushed
      // on reconnect. Task 7: record WHY so the failure isn't silent.
      onSyncResult?.call(SyncStatus.classify(e));
      return queued;
    }
  }

  @override
  Future<String> transcribe({required List<int> audio, String? language}) async {
    // A real provider/no-key failure MUST reach the UI (the field shows the reason). Only a genuine
    // offline/transport failure degrades — to a network-kind TranscribeException, not silent empty.
    try {
      return await _inner.transcribe(audio: audio, language: language);
    } on TranscribeException {
      rethrow;
    } catch (_) {
      throw const TranscribeException('offline', kind: TranscribeErrorKind.network);
    }
  }

  @override
  Future<List<Idea>> captures({DateTime? day}) async {
    try {
      final fresh = await _inner.captures(day: day);
      if (day == null) {
        cache.putFeed(fresh); // refresh cache, preserving unflushed queued rows
        return cache.feed();
      }
      // A date-scoped view must NOT overwrite/LRU-evict the live feed cache, but it must still surface
      // any unflushed queued capture that belongs to this day — never hide a made capture (offline-first;
      // the live feed + offline day-view both keep it). Deduped against the server rows, queued first.
      //
      // Scope the SERVER rows to the chosen day ourselves rather than trusting the wire window: a backend
      // that ignores `?start&end` (e.g. a stale deploy) returns every day's captures, which would leak the
      // whole feed into the day-view. `sameLocalDay` agrees with the UTC window the request asked for, so a
      // correctly-windowing server loses nothing — this only drops rows that fell outside the chosen day.
      final freshForDay = fresh.where((i) => sameLocalDay(i.capturedAt, day));
      final ids = freshForDay.map((i) => i.id).toSet();
      final queuedForDay = cache.outbound()
          .map((q) => q.idea)
          .where((i) => sameLocalDay(i.capturedAt, day) && !ids.contains(i.id));
      return [...queuedForDay, ...freshForDay];
    } catch (_) {
      final all = cache.feed(); // degrade to cache (S4-R56)
      return day == null ? all : all.where((i) => sameLocalDay(i.capturedAt, day)).toList();
    }
  }

  // Task 10/11: detail stays a pass-through; edit/delete are offline-queued —
  // buffered locally + applied optimistically to the cached feed BEFORE the
  // network, then flushed (delete-wins) on reconnect.
  @override
  Future<Idea?> captureDetail(String id) => _inner.captureDetail(id);

  @override
  Future<void> updateCapture(String id, {String? body, double? lat, double? lng, double? locAccuracyM}) async {
    // Buffer + optimistically update the cached feed FIRST (collapses with any
    // queued mutation for this id), then try the network. On a clean online
    // success drop the queued row so it isn't re-flushed later.
    cache.enqueueEdit(id, body: body, lat: lat, lng: lng, locAccuracyM: locAccuracyM);
    try {
      await _inner.updateCapture(id, body: body, lat: lat, lng: lng, locAccuracyM: locAccuracyM);
      cache.dropMutation(id, 'edit');
      onSyncResult?.call(null);
    } catch (e) {
      // Offline / server unreachable / unauthorized: stays queued for the next
      // flush; record WHY so the failure isn't silent (Task 7).
      onSyncResult?.call(SyncStatus.classify(e));
    }
  }

  @override
  Future<void> deleteCapture(String id) async {
    cache.enqueueDelete(id); // delete wins over any queued edit + drops the feed row
    try {
      await _inner.deleteCapture(id);
      cache.dropMutation(id, 'delete');
      onSyncResult?.call(null);
    } catch (e) {
      onSyncResult?.call(SyncStatus.classify(e));
    }
  }

  // ── Wiki ───────────────────────────────────────────────────────────────

  @override
  Future<List<WikiPage>> wikiIndex() async {
    try {
      final fresh = await _inner.wikiIndex();
      cache.putWikiIndex(fresh);
      return fresh;
    } catch (_) {
      final cached = cache.wikiIndex();
      if (cached.isNotEmpty) return cached;
      rethrow;
    }
  }

  @override
  Future<WikiPage?> wikiPage(String slug) async {
    try {
      final page = await _inner.wikiPage(slug);
      if (page != null) cache.putWikiPage(page);
      return page ?? cache.wikiPage(slug);
    } catch (_) {
      return cache.wikiPage(slug); // last-opened page renders offline
    }
  }

  // ── outbound flush ───────────────────────────────────────────────────────

  /// S4-R26: flush the outbound queue. Triggered on reconnect + app resume.
  /// Idempotent by client id — a duplicate call never double-POSTs.
  Future<void> flushOutbox() => _flushVia();

  /// POST each queued capture exactly once via the wrapped client, THEN flush the
  /// queued edit/delete mutations (Task 11). The cache's [IdeaCache.flush] /
  /// [IdeaCache.flushMutations] dequeue only on a successful POST, so a throwing
  /// call leaves the rest queued for the next attempt. Captures flush first so a
  /// mutation always targets a row the server already knows about; the queued
  /// row's [OutboundCapture.idempotencyKey] rides along so the server dedups a
  /// row that was also POSTed by the immediate capture path (Task 6).
  Future<void> _flushVia() async {
    await cache.flush((c) async {
      // LOC-R1: re-send the queued capture's location so an offline capture
      // keeps its action-time fix when it finally flushes to the server.
      // Part F / LOC-R3: re-send the queued row's ORIGINAL action-time
      // `capturedAt` (stamped at enqueue) so a capture made at 08:30 and flushed
      // at noon is stamped 08:30 server-side, not the flush time.
      await _inner.capture(
        body: c.idea.body,
        origin: c.idea.origin,
        idempotencyKey: c.idempotencyKey,
        lat: c.idea.lat,
        lng: c.idea.lng,
        accuracyM: c.idea.locAccuracyM,
        capturedAt: c.idea.capturedAt,
      );
    });
    await cache.flushMutations((m) async {
      if (m.kind == 'delete') {
        await _inner.deleteCapture(m.id);
      } else {
        await _inner.updateCapture(m.id, body: m.body, lat: m.lat, lng: m.lng, locAccuracyM: m.locAccuracyM);
      }
    });
  }

  // ── pass-through (unchanged) ──────────────────────────────────────────────

  @override
  Stream<ApiConnectionState> get connectionState => _inner.connectionState;

  @override
  Stream<RuntimeEventEnvelope> events({required String threadId}) =>
      _inner.events(threadId: threadId);

  @override
  Future<Map<String, dynamic>> request(String method,
          {Map<String, dynamic> params = const {}}) =>
      _inner.request(method, params: params);

  @override
  Stream<SseEvent> recallStream(
    String question, {
    String? threadId,
    String? provider,
    String? model,
    String? effort,
  }) =>
      _inner.recallStream(
        question,
        threadId: threadId,
        provider: provider,
        model: model,
        effort: effort,
      );

  @override
  Future<List<ConversationSummary>> listConversations() => _inner.listConversations();

  @override
  Future<List<ConversationMessage>> getConversationMessages(String threadId) =>
      _inner.getConversationMessages(threadId);

  @override
  Future<List<Approval>> approvals() => _inner.approvals();

  @override
  Future<void> respondApproval(String id, bool approve) =>
      _inner.respondApproval(id, approve);

  @override
  Future<int> runIngest() => _inner.runIngest();

  @override
  Future<List<JobRow>> jobs() => _inner.jobs();

  @override
  Future<int> todayCostMicros() => _inner.todayCostMicros();

  @override
  Stream<SseEvent> dreamEvents(String jobId) => _inner.dreamEvents(jobId);

  @override
  Future<void> cancelJob(String jobId) => _inner.cancelJob(jobId);

  @override
  Future<List<ProviderCredential>> credentials() => _inner.credentials();

  @override
  Future<ProviderCredential> putKey(String provider, String key) =>
      _inner.putKey(provider, key);

  @override
  Future<void> deleteKey(String provider) => _inner.deleteKey(provider);

  @override
  Future<List<String>> fetchModels(String provider) =>
      _inner.fetchModels(provider);

  @override
  Future<String?> activeModel(String provider) => _inner.activeModel(provider);

  @override
  Future<void> setActiveModel(String provider, String model) =>
      _inner.setActiveModel(provider, model);

  @override
  Future<List<CostLedgerRow>> costLedger() => _inner.costLedger();

  @override
  Future<bool> serverDream() => _inner.serverDream();

  @override
  Future<void> setServerDream(bool on) => _inner.setServerDream(on);

  @override
  Future<void> deleteAccount() async {
    await _inner.deleteAccount();
    // This decorator owns the local cache — wipe it so a deleted account leaves
    // no residual data on the device.
    cache.clear();
  }

  // ── Sync (Phase 1) — delegate to the inner client; the SyncClient owns the
  // applyDelta-into-[cache] policy (which the bootstrap exposes via `cache`). ──

  @override
  Future<DeviceReg> registerDevice(String platform) =>
      _inner.registerDevice(platform);

  @override
  Future<SyncDelta> pullSync({required int since}) =>
      _inner.pullSync(since: since);

  @override
  Future<AppReleaseManifest> fetchReleaseManifest(String platform) =>
      _inner.fetchReleaseManifest(platform); // AU-R15: metadata read; no offline caching needed

  @override
  Future<void> dispose() => _inner.dispose();
}
