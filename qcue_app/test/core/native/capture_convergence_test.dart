// QCue S5-R2/R10/R11/R38: every native capture path (share, widget) feeds the
// SAME idempotent, offline-safe capture path. Here the facades' [Enqueue] seam is
// bound to the REAL OfflineAwareApiClient.capture (persist-local-before-network),
// and the facades are driven through their ACTUAL event channels, proving:
//   - a share/widget enqueue lands in the offline queue + cached feed EVEN OFFLINE
//     (S5-R11), with `origin` recorded for S2 fencing (S5-R10);
//   - a background flush drains the queue idempotently (S5-R38) and never
//     double-POSTs on a repeated flush.
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/models/recall_conversation.dart';
import 'package:qcue_app/core/models/runtime_event.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/native/background/background_flush.dart';
import 'package:qcue_app/core/native/channels.dart';
import 'package:qcue_app/core/native/protocol/capture_enqueue.dart';
import 'package:qcue_app/core/native/share/share_channel.dart';
import 'package:qcue_app/core/native/widget/widget_channel.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/models/app_release_manifest.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';
import 'package:qcue_app/core/offline/offline_api_client.dart';
import 'package:qcue_app/core/sync/sync_dtos.dart';

/// A seeded inner client whose network can be flipped offline; records the
/// (body, origin) pairs the server "received" so we can assert exactly-once.
class _Inner implements QcueApiClient {
  final StubApiClient _stub = StubApiClient.seeded();
  bool online = true;
  final List<(String, String)> received = [];

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
    if (!online) throw Exception('offline');
    received.add((body, origin));
    return _stub.capture(
        body: body,
        origin: origin,
        lat: lat,
        lng: lng,
        accuracyM: accuracyM,
        capturedAt: capturedAt);
  }

  @override
  Future<List<Idea>> captures({DateTime? day}) => _stub.captures(day: day);
  @override
  Future<Idea?> captureDetail(String id) => _stub.captureDetail(id);
  @override
  Future<void> updateCapture(String id, {String? body, double? lat, double? lng, double? locAccuracyM}) =>
      _stub.updateCapture(id, body: body, lat: lat, lng: lng, locAccuracyM: locAccuracyM);
  @override
  Future<void> deleteCapture(String id) => _stub.deleteCapture(id);
  @override
  Future<String> transcribe({required List<int> audio, String? language}) =>
      _stub.transcribe(audio: audio, language: language);
  @override
  Future<List<WikiPage>> wikiIndex() => _stub.wikiIndex();
  @override
  Future<WikiPage?> wikiPage(String slug) => _stub.wikiPage(slug);
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
  Stream<SseEvent> recallStream(String q,
          {String? threadId, String? provider, String? model, String? effort}) =>
      _stub.recallStream(q,
          threadId: threadId, provider: provider, model: model, effort: effort);
  @override
  Future<List<ConversationSummary>> listConversations() => _stub.listConversations();
  @override
  Future<List<ConversationMessage>> getConversationMessages(String threadId) =>
      _stub.getConversationMessages(threadId);
  @override
  Future<List<Approval>> approvals() => _stub.approvals();
  @override
  Future<void> respondApproval(String id, bool a) =>
      _stub.respondApproval(id, a);
  @override
  Future<int> runIngest() => _stub.runIngest();
  @override
  Future<List<JobRow>> jobs() => _stub.jobs();
  @override
  Future<int> todayCostMicros() => _stub.todayCostMicros();
  @override
  Stream<SseEvent> dreamEvents(String j) => _stub.dreamEvents(j);
  @override
  Future<void> cancelJob(String j) => _stub.cancelJob(j);
  @override
  Future<List<ProviderCredential>> credentials() => _stub.credentials();
  @override
  Future<ProviderCredential> putKey(String p, String k) => _stub.putKey(p, k);
  @override
  Future<void> deleteKey(String p) => _stub.deleteKey(p);
  @override
  Future<void> deleteAccount() => _stub.deleteAccount();
  @override
  Future<List<String>> fetchModels(String p) => _stub.fetchModels(p);
  @override
  Future<String?> activeModel(String p) => _stub.activeModel(p);
  @override
  Future<void> setActiveModel(String p, String m) => _stub.setActiveModel(p, m);
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

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  late _Inner inner;
  late OfflineAwareApiClient api;
  late Enqueue enqueue;

  void emit(String channel, Object? e) => messenger.handlePlatformMessage(
        channel,
        const StandardMethodCodec().encodeSuccessEnvelope(e),
        (_) {},
      );

  setUp(() {
    inner = _Inner();
    api = OfflineAwareApiClient(
        inner, IdeaCache(InMemoryCacheStore(), feedCap: 50));
    // the single seam every native facade uses (bootstrap binds it identically).
    enqueue = (req) => api.capture(body: req.body, origin: req.origin);
    for (final ch in [
      QcueChannels.share,
      QcueChannels.shareEvents,
      QcueChannels.widget,
      QcueChannels.widgetEvents,
    ]) {
      messenger.setMockMethodCallHandler(MethodChannel(ch), (_) async => null);
    }
  });

  tearDown(() {
    for (final ch in [
      QcueChannels.share,
      QcueChannels.shareEvents,
      QcueChannels.widget,
      QcueChannels.widgetEvents,
    ]) {
      messenger.setMockMethodCallHandler(MethodChannel(ch), null);
    }
  });

  test('S5-R2/R10/R11: share + widget both converge OFFLINE with origin',
      () async {
    inner.online = false; // fully offline

    final share = ShareChannel(enqueue: enqueue)..start();
    final widget = WidgetChannel(enqueue: enqueue, onDeepLink: (_) {})..start();
    await pumpEventQueue();

    emit(QcueChannels.shareEvents,
        {'url': 'https://a', 'sourceApp': 'safari'});
    emit(QcueChannels.widgetEvents,
        {'action': 'quickCapture', 'args': {'body': 'quick'}});
    await pumpEventQueue();

    // both queued in the offline cache (persist-before-network); nothing POSTed.
    final queued = api.cache.outbound();
    expect(queued, hasLength(2));
    expect(queued.map((q) => q.idea.origin).toSet(),
        {'share:web:safari', 'capture:widget'});
    expect(
        queued.every((q) => q.idea.ingestState == IngestState.pending), isTrue);
    expect(inner.received, isEmpty);

    await share.dispose();
    await widget.dispose();
  });

  test('S5-R38: a background flush drains the queue exactly once (idempotent)',
      () async {
    inner.online = false;
    final share = ShareChannel(enqueue: enqueue)..start();
    await pumpEventQueue();
    emit(QcueChannels.shareEvents, {'text': 'hello', 'sourceApp': 'notes'});
    await pumpEventQueue();
    expect(api.cache.outbound(), hasLength(1));

    inner.online = true;
    final bg = BackgroundFlush(flush: api.flushOutbox);
    await bg.runFlush();
    await bg.runFlush(); // repeated flush must NOT double-POST (S5-R38)

    expect(inner.received, [('hello', 'share:text:notes')]);
    expect(api.cache.outbound(), isEmpty);

    await share.dispose();
  });
}
