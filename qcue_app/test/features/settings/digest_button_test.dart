// QCue DIG-R6: the one-click Digest action now lives on the SETTINGS screen (moved off Wiki). It calls
// runIngest(), disables while in flight, and surfaces the enqueued count. This also guards that the Wiki
// screen no longer carries a digest affordance.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/models/recall_conversation.dart';
import 'package:qcue_app/core/models/runtime_event.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/models/app_release_manifest.dart';
import 'package:qcue_app/core/sync/sync_dtos.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:qcue_app/features/settings/settings_screen.dart';
import 'package:qcue_app/features/wiki/wiki_screen.dart';

/// Records runIngest() calls and returns a fixed enqueued count; everything else delegates to a seeded
/// stub so Settings/Wiki load normally.
class DigestRecordingApi implements QcueApiClient {
  final _d = StubApiClient.seeded();
  int calls = 0;

  @override
  Future<int> runIngest() async {
    calls++;
    return 3;
  }

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
  Future<String> transcribe({required List<int> audio, String? language}) =>
      _d.transcribe(audio: audio, language: language);
  @override
  Stream<SseEvent> recallStream(String question,
          {String? threadId, String? provider, String? model, String? effort}) =>
      _d.recallStream(question,
          threadId: threadId, provider: provider, model: model, effort: effort);
  @override
  Future<List<ConversationSummary>> listConversations() => _d.listConversations();
  @override
  Future<List<ConversationMessage>> getConversationMessages(String threadId) =>
      _d.getConversationMessages(threadId);
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
  Future<List<WikiPage>> wikiIndex() => _d.wikiIndex();
  @override
  Future<WikiPage?> wikiPage(String slug) => _d.wikiPage(slug);
  @override
  Future<List<Approval>> approvals() => _d.approvals();
  @override
  Future<void> respondApproval(String id, bool approve) =>
      _d.respondApproval(id, approve);
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

Widget _settingsHost(QcueApiClient api) => ProviderScope(
      overrides: [apiClientProvider.overrideWithValue(api)],
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const Scaffold(body: SettingsScreen()),
      ),
    );

void main() {
  testWidgets('Settings Digest row calls runIngest and shows the enqueued count',
      (tester) async {
    // Settings is a ListView — a tall viewport renders all rows without scrolling.
    tester.view.physicalSize = const Size(1200, 4000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = DigestRecordingApi();
    await tester.pumpWidget(_settingsHost(api));
    await tester.pumpAndSettle();

    final row = find.byKey(const ValueKey('settings-digest'));
    expect(row, findsOneWidget);
    await tester.tap(row);
    await tester.pump(); // start the async run (DigestRunning)
    await tester.pumpAndSettle(); // settle to DigestDone
    expect(api.calls, 1, reason: 'one runIngest() call per tap');
    expect(find.textContaining('3'), findsWidgets); // enqueued count surfaced
  });

  testWidgets('the Wiki screen no longer carries a digest affordance', (tester) async {
    await tester.pumpWidget(ProviderScope(
      overrides: [apiClientProvider.overrideWithValue(DigestRecordingApi())],
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: Scaffold(body: WikiScreen(onOpenPage: (_) {})),
      ),
    ));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('wiki-digest')), findsNothing);
    expect(find.byKey(const ValueKey('settings-digest')), findsNothing);
  });
}
