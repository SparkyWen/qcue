// QCue: the recall composer's ChatGPT-style Intelligence + Model selector. Verifies the pill
// sits in the composer, Level 1 lists the 5 effort levels (choosing one updates the sticky
// selection), the model entry opens Level 2 (choosing a model updates provider+model and the
// pill label), it's disabled while a turn streams, and the selection forwards into recallStream.
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/recall/recall_provider.dart';
import 'package:qcue_app/features/recall/recall_screen.dart';
import 'package:qcue_app/features/recall/recall_selection.dart';

/// Records the per-recall override passed to recallStream; everything else is
/// stubbed via noSuchMethod (only recallStream + an empty stream are exercised).
class _RecordingClient implements QcueApiClient {
  String? provider, model, effort;
  final controller = StreamController<SseEvent>.broadcast();

  @override
  Stream<SseEvent> recallStream(
    String question, {
    String? threadId,
    String? provider,
    String? model,
    String? effort,
  }) {
    this.provider = provider;
    this.model = model;
    this.effort = effort;
    return controller.stream;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      super.noSuchMethod(invocation);
}

Widget _app(ProviderContainer container) => UncontrolledProviderScope(
      container: container,
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const Scaffold(body: RecallScreen()),
      ),
    );

void main() {
  testWidgets('two-level selector: pick effort, then pick a model',
      (tester) async {
    final container = ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(StubApiClient.seeded()),
    ]);
    addTearDown(container.dispose);
    await tester.pumpWidget(_app(container));
    await tester.pumpAndSettle();

    final pill = find.byKey(const ValueKey('intelligence-selector'));
    expect(pill, findsOneWidget);

    // Level 1 opens with the 5 ChatGPT effort levels.
    await tester.tap(pill);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('intelligence-sheet')), findsOneWidget);
    for (final wire in ['minimal', 'medium', 'high', 'xhigh', 'max']) {
      expect(find.byKey(ValueKey('effort-opt-$wire')), findsOneWidget);
    }

    // Choosing "High" updates the sticky selection and closes Level 1.
    await tester.tap(find.byKey(const ValueKey('effort-opt-high')));
    await tester.pumpAndSettle();
    expect(container.read(recallSelectionProvider).effort, RecallEffort.high);
    expect(find.byKey(const ValueKey('intelligence-sheet')), findsNothing);

    // Re-open and drill into the model list (Level 2).
    await tester.tap(pill);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('intelligence-model-entry')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('model-sheet')), findsOneWidget);
    expect(find.byKey(const ValueKey('model-row-gpt-5.5')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('model-row-gpt-5.5')));
    await tester.pumpAndSettle();
    expect(container.read(recallSelectionProvider).model, 'gpt-5.5');
    expect(container.read(recallSelectionProvider).provider, 'openai');

    // The pill label now reflects both axes.
    expect(find.text('GPT-5.5 · High'), findsOneWidget);
  });

  testWidgets('the selector is disabled while a turn streams', (tester) async {
    final rec = _RecordingClient();
    final container =
        ProviderContainer(overrides: [apiClientProvider.overrideWithValue(rec)]);
    addTearDown(container.dispose);
    await tester.pumpWidget(_app(container));
    await tester.pumpAndSettle();

    await tester.enterText(
        find.byKey(const ValueKey('recall-input')), 'a question');
    await tester.testTextInput.receiveAction(TextInputAction.send);
    rec.controller.add(const SessionStarted('th'));
    rec.controller.add(const MessageDelta('partial'));
    await tester.pump();

    // Tapping the pill while streaming must not open the popover.
    await tester.tap(find.byKey(const ValueKey('intelligence-selector')));
    await tester.pump();
    expect(find.byKey(const ValueKey('intelligence-sheet')), findsNothing);
    await rec.controller.close();
    await tester.pumpAndSettle();
  });

  test('ask forwards the selection into recallStream', () {
    final rec = _RecordingClient();
    final container =
        ProviderContainer(overrides: [apiClientProvider.overrideWithValue(rec)]);
    addTearDown(container.dispose);

    container
        .read(recallSelectionProvider.notifier)
        .setModel('openai', 'gpt-5.5');
    container.read(recallSelectionProvider.notifier).setEffort(RecallEffort.high);

    container.read(recallProvider.notifier).ask('what did I decide?');

    expect(rec.provider, 'openai');
    expect(rec.model, 'gpt-5.5');
    expect(rec.effort, 'high');
  });
}
