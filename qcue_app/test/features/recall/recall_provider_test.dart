import 'dart:async';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/models/recall_conversation.dart';
import 'package:qcue_app/core/models/runtime_event.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/models/app_release_manifest.dart';
import 'package:qcue_app/core/sync/cache_revision.dart';
import 'package:qcue_app/core/sync/sync_dtos.dart';
import 'package:qcue_app/features/recall/recall_provider.dart';

/// Records the threadId passed to recallStream and returns a controllable stream;
/// everything else delegates to a seeded stub (REC-R7).
class RecordingRecallClient implements QcueApiClient {
  final _d = StubApiClient.seeded();
  StreamController<SseEvent>? _ctrl;
  String? lastThreadId;

  @override
  Stream<SseEvent> recallStream(
    String question, {
    String? threadId,
    String? provider,
    String? model,
    String? effort,
  }) {
    lastThreadId = threadId;
    _ctrl = StreamController<SseEvent>();
    return _ctrl!.stream;
  }

  void emit(SseEvent e) => _ctrl!.add(e);
  Future<void> settle() => Future<void>.delayed(Duration.zero);

  @override
  Stream<ApiConnectionState> get connectionState => _d.connectionState;
  @override
  Stream<RuntimeEventEnvelope> events({required String threadId}) =>
      _d.events(threadId: threadId);
  @override
  Future<Map<String, dynamic>> request(String method,
          {Map<String, dynamic> params = const {}}) =>
      _d.request(method, params: params);
  @override
  Future<Idea> capture(
          {required String body,
          required String origin,
          String? idempotencyKey,
          double? lat,
          double? lng,
          double? accuracyM,
          DateTime? capturedAt}) =>
      _d.capture(
          body: body,
          origin: origin,
          idempotencyKey: idempotencyKey,
          lat: lat,
          lng: lng,
          accuracyM: accuracyM,
          capturedAt: capturedAt);
  @override
  Future<List<Idea>> captures({DateTime? day}) => _d.captures(day: day);
  @override
  Future<Idea?> captureDetail(String id) => _d.captureDetail(id);
  @override
  Future<void> updateCapture(String id, {String? body, double? lat, double? lng, double? locAccuracyM}) =>
      _d.updateCapture(id, body: body, lat: lat, lng: lng, locAccuracyM: locAccuracyM);
  @override
  Future<void> deleteCapture(String id) => _d.deleteCapture(id);
  @override
  Future<String> transcribe({required List<int> audio, String? language}) =>
      _d.transcribe(audio: audio, language: language);
  @override
  Future<List<WikiPage>> wikiIndex() => _d.wikiIndex();
  @override
  Future<WikiPage?> wikiPage(String slug) => _d.wikiPage(slug);
  @override
  Future<List<ConversationSummary>> listConversations() => _d.listConversations();
  @override
  Future<List<ConversationMessage>> getConversationMessages(String threadId) =>
      _d.getConversationMessages(threadId);
  @override
  Future<List<Approval>> approvals() => _d.approvals();
  @override
  Future<void> respondApproval(String id, bool approve) =>
      _d.respondApproval(id, approve);
  @override
  Future<int> runIngest() => _d.runIngest();
  @override
  Future<List<JobRow>> jobs() => _d.jobs();
  @override
  Future<int> todayCostMicros() => _d.todayCostMicros();
  @override
  Stream<SseEvent> dreamEvents(String jobId) => _d.dreamEvents(jobId);
  @override
  Future<void> cancelJob(String jobId) => _d.cancelJob(jobId);
  @override
  Future<List<ProviderCredential>> credentials() => _d.credentials();
  @override
  Future<ProviderCredential> putKey(String provider, String key) =>
      _d.putKey(provider, key);
  @override
  Future<void> deleteKey(String provider) => _d.deleteKey(provider);
  @override
  Future<void> deleteAccount() => _d.deleteAccount();
  @override
  Future<List<String>> fetchModels(String provider) => _d.fetchModels(provider);
  @override
  Future<String?> activeModel(String provider) => _d.activeModel(provider);
  @override
  Future<void> setActiveModel(String provider, String model) =>
      _d.setActiveModel(provider, model);
  @override
  Future<List<CostLedgerRow>> costLedger() => _d.costLedger();
  @override
  Future<bool> serverDream() => _d.serverDream();
  @override
  Future<void> setServerDream(bool on) => _d.setServerDream(on);
  @override
  Future<DeviceReg> registerDevice(String platform) => _d.registerDevice(platform);
  @override
  Future<SyncDelta> pullSync({required int since}) => _d.pullSync(since: since);
  @override
  Future<AppReleaseManifest> fetchReleaseManifest(String platform) =>
      _d.fetchReleaseManifest(platform);

  @override
  Future<void> dispose() => _d.dispose();
}

void main() {
  test('ask appends turns, captures threadId, and reuses it on continue', () async {
    final fake = RecordingRecallClient();
    final container = ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(fake),
    ]);
    addTearDown(container.dispose);

    final notifier = container.read(recallProvider.notifier);
    notifier.ask('first question');
    fake.emit(const SessionStarted('th-real'));
    fake.emit(const MessageDelta('A1'));
    fake.emit(const DoneEvent());
    await fake.settle();

    var convo = container.read(recallProvider)!;
    expect(convo.turns.length, 1);
    expect(convo.turns.first.answer, 'A1');
    expect(convo.threadId, 'th-real');

    // continue: the SAME thread id is reused (REC-R7).
    notifier.ask('second question');
    expect(fake.lastThreadId, 'th-real');
    fake.emit(const MessageDelta('A2'));
    fake.emit(const DoneEvent());
    await fake.settle();

    convo = container.read(recallProvider)!;
    expect(convo.turns.length, 2, reason: 'a continue appends, never replaces');
    expect(convo.turns.last.answer, 'A2');
  });

  test('a finished recall turn bumps the cache revision so history refreshes (RC3)',
      () async {
    final fake = RecordingRecallClient();
    final container = ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(fake),
    ]);
    addTearDown(container.dispose);

    expect(container.read(cacheRevisionProvider), 0);
    container.read(recallProvider.notifier).ask('q');
    fake.emit(const SessionStarted('th-1'));
    fake.emit(const MessageDelta('A'));
    fake.emit(const DoneEvent());
    await fake.settle();

    expect(container.read(cacheRevisionProvider), greaterThan(0),
        reason: 'a newly-created thread must surface in the history drawer without relaunch');
  });
}
