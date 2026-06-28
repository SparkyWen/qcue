// QCue S4-R37/R38/R39: the Recall chat. Submitting a question calls
// QcueApiClient.recallStream() and renders the answer streamed token-by-token
// (message_delta), with tappable inline [[links]], citation chips (from
// citation events), and a collapsed-by-default reasoning disclosure (D18).
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/models/recall_conversation.dart';
import 'package:qcue_app/core/models/runtime_event.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/models/app_release_manifest.dart';
import 'package:qcue_app/core/sync/sync_dtos.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/recall/recall_screen.dart';
import 'package:qcue_app/widgets/citation_chip.dart';
import 'package:qcue_app/widgets/reasoning_disclosure.dart';
import 'package:qcue_app/widgets/streaming_text.dart';

/// A controllable client whose recall stream we drive event-by-event, so the
/// test asserts the answer grows *incrementally* (not just at completion).
/// Everything else delegates to a seeded stub.
class ScriptedApiClient implements QcueApiClient {
  @override
  Future<SttProviders> sttProviders() async =>
      const SttProviders(selected: null, available: [], allCapable: []);
  @override
  Future<void> setSttProvider(String? provider) async {}

  final _delegate = StubApiClient.seeded();
  final controller = StreamController<SseEvent>();

  String? lastProvider;
  String? lastModel;
  String? lastEffort;

  @override
  Stream<SseEvent> recallStream(
    String question, {
    String? threadId,
    String? provider,
    String? model,
    String? effort,
  }) {
    lastProvider = provider;
    lastModel = model;
    lastEffort = effort;
    return controller.stream;
  }

  @override
  Future<List<ConversationSummary>> listConversations() => _delegate.listConversations();
  @override
  Future<List<ConversationMessage>> getConversationMessages(String threadId) =>
      _delegate.getConversationMessages(threadId);

  @override
  Stream<ApiConnectionState> get connectionState => _delegate.connectionState;
  @override
  Stream<RuntimeEventEnvelope> events({required String threadId}) =>
      _delegate.events(threadId: threadId);
  @override
  Future<Map<String, dynamic>> request(String method,
          {Map<String, dynamic> params = const {}}) =>
      _delegate.request(method, params: params);
  @override
  Future<String> transcribe({required List<int> audio, String? language}) =>
      _delegate.transcribe(audio: audio, language: language);
  @override
  Future<Idea> capture({
    required String body,
    required String origin,
    String? idempotencyKey,
    double? lat,
    double? lng,
    double? accuracyM,
    DateTime? capturedAt,
  }) =>
      _delegate.capture(
          body: body,
          origin: origin,
          idempotencyKey: idempotencyKey,
          lat: lat,
          lng: lng,
          accuracyM: accuracyM,
          capturedAt: capturedAt);
  @override
  Future<List<Idea>> captures({DateTime? day}) => _delegate.captures(day: day);
  @override
  Future<Idea?> captureDetail(String id) => _delegate.captureDetail(id);
  @override
  Future<void> updateCapture(String id, {String? body, double? lat, double? lng, double? locAccuracyM}) =>
      _delegate.updateCapture(id, body: body, lat: lat, lng: lng, locAccuracyM: locAccuracyM);
  @override
  Future<void> deleteCapture(String id) => _delegate.deleteCapture(id);
  @override
  Future<List<WikiPage>> wikiIndex() => _delegate.wikiIndex();
  @override
  Future<WikiPage?> wikiPage(String slug) => _delegate.wikiPage(slug);
  @override
  Future<List<Approval>> approvals() => _delegate.approvals();
  @override
  Future<void> respondApproval(String id, bool approve) =>
      _delegate.respondApproval(id, approve);
  @override
  Future<int> runIngest() => _delegate.runIngest();
  @override
  Future<List<JobRow>> jobs() => _delegate.jobs();
  @override
  Future<int> todayCostMicros() => _delegate.todayCostMicros();
  @override
  Stream<SseEvent> dreamEvents(String jobId) => _delegate.dreamEvents(jobId);
  @override
  Future<void> cancelJob(String jobId) => _delegate.cancelJob(jobId);
  @override
  Future<List<ProviderCredential>> credentials() => _delegate.credentials();
  @override
  Future<ProviderCredential> putKey(String provider, String key) =>
      _delegate.putKey(provider, key);
  @override
  Future<void> deleteKey(String provider) => _delegate.deleteKey(provider);
  @override
  Future<void> deleteAccount() => _delegate.deleteAccount();
  @override
  Future<List<String>> fetchModels(String provider) =>
      _delegate.fetchModels(provider);
  @override
  Future<String?> activeModel(String provider) =>
      _delegate.activeModel(provider);
  @override
  Future<void> setActiveModel(String provider, String model) =>
      _delegate.setActiveModel(provider, model);
  @override
  Future<List<CostLedgerRow>> costLedger() => _delegate.costLedger();
  @override
  Future<bool> serverDream() => _delegate.serverDream();
  @override
  Future<void> setServerDream(bool on) => _delegate.setServerDream(on);
  @override
  Future<DeviceReg> registerDevice(String platform) =>
      _delegate.registerDevice(platform);
  @override
  Future<SyncDelta> pullSync({required int since}) =>
      _delegate.pullSync(since: since);
  @override
  Future<AppReleaseManifest> fetchReleaseManifest(String platform) =>
      _delegate.fetchReleaseManifest(platform);

  @override
  Future<void> dispose() => _delegate.dispose();
}

Widget _app(QcueApiClient api) => ProviderScope(
      overrides: [apiClientProvider.overrideWithValue(api)],
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const Scaffold(body: RecallScreen()),
      ),
    );

Future<void> _ask(WidgetTester tester, String q) async {
  await tester.enterText(find.byKey(const ValueKey('recall-input')), q);
  await tester.testTextInput.receiveAction(TextInputAction.send);
  await tester.pump();
}

void main() {
  testWidgets('empty state prompts the user to ask', (tester) async {
    await tester.pumpWidget(_app(StubApiClient.seeded()));
    await tester.pumpAndSettle();
    expect(find.textContaining('Ask'), findsWidgets);
  });

  testWidgets('renders message deltas incrementally as they stream',
      (tester) async {
    final api = ScriptedApiClient();
    await tester.pumpWidget(_app(api));
    await _ask(tester, 'what about embeddings?');

    api.controller.add(const SessionStarted('th-1'));
    api.controller.add(const MessageDelta('You decided '));
    await tester.pump();
    expect(find.textContaining('You decided'), findsOneWidget);
    // the rest of the answer is NOT here yet
    expect(find.textContaining('against embeddings'), findsNothing);

    api.controller.add(const MessageDelta('against embeddings.'));
    await tester.pump();
    expect(find.textContaining('against embeddings'), findsOneWidget);

    await api.controller.close();
    await tester.pumpAndSettle();
  });

  testWidgets('citation events render citation chips', (tester) async {
    final api = ScriptedApiClient();
    await tester.pumpWidget(_app(api));
    await _ask(tester, 'q');
    api.controller.add(const MessageDelta('answer'));
    api.controller.add(const CitationEvent(
        Citation(relPath: 'source.md', startLine: 42, endLine: 42)));
    await tester.pump();
    expect(find.byType(CitationChip), findsOneWidget);
    expect(find.text('source.md:42'), findsOneWidget);
    await api.controller.close();
    await tester.pumpAndSettle();
  });

  testWidgets('composer send is disabled while a turn streams (S4-R37)',
      (tester) async {
    final api = ScriptedApiClient();
    await tester.pumpWidget(_app(api));
    await _ask(tester, 'q');
    api.controller.add(const SessionStarted('th'));
    api.controller.add(const MessageDelta('partial'));
    await tester.pump();
    // mid-stream: send is disabled (null onPressed greys the icon).
    final btn =
        tester.widget<IconButton>(find.byKey(const ValueKey('recall-send')));
    expect(btn.onPressed, isNull, reason: 'send disabled while streaming');
    // after the turn completes, the composer re-enables.
    await api.controller.close();
    await tester.pumpAndSettle();
    final btn2 =
        tester.widget<IconButton>(find.byKey(const ValueKey('recall-send')));
    expect(btn2.onPressed, isNotNull, reason: 'send re-enabled after stream');
  });

  testWidgets('reasoning is rendered collapsed by default (D18)',
      (tester) async {
    final api = ScriptedApiClient();
    await tester.pumpWidget(_app(api));
    await _ask(tester, 'q');
    api.controller.add(const ReasoningDelta('secret chain of thought'));
    api.controller.add(const MessageDelta('answer'));
    await tester.pump();
    // the disclosure exists, but the reasoning text is hidden until expanded
    expect(find.byType(ReasoningDisclosure), findsOneWidget);
    expect(find.text('secret chain of thought'), findsNothing);
    await tester.tap(find.text('Reasoning'));
    await tester.pumpAndSettle();
    expect(find.text('secret chain of thought'), findsOneWidget);
    await api.controller.close();
    await tester.pumpAndSettle();
  });

  testWidgets('the full seeded recall renders streamed text + a chip + a link',
      (tester) async {
    await tester.pumpWidget(_app(StubApiClient.seeded()));
    await _ask(tester, 'what did I decide about embeddings?');
    await tester.pumpAndSettle();
    // assembled answer present
    expect(find.byType(StreamingText), findsWidgets);
    expect(find.textContaining('You decided'), findsOneWidget);
    // citation chip from the seeded citation event
    expect(find.byType(CitationChip), findsOneWidget);
    // inline [[Recall Architecture]] link is linkText-colored (delegated)
    final link = qThemeColors(QThemeId.cleanLight)[QToken.linkText];
    final richTexts = tester.widgetList<RichText>(find.byType(RichText));
    final hasLink = richTexts.any((rt) {
      var found = false;
      void visit(InlineSpan s) {
        if (s is TextSpan) {
          if ((s.text ?? '').contains('Recall Architecture') &&
              s.style?.color == link) {
            found = true;
          }
          for (final c in s.children ?? const <InlineSpan>[]) {
            visit(c);
          }
        }
      }

      visit(rt.text);
      return found;
    });
    expect(hasLink, isTrue);
  });
}
