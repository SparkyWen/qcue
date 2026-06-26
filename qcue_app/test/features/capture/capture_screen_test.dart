// QCue S4-R30/R31/R32: the Capture feed screen. Submitting via the pinned field
// calls QcueApiClient.capture() and appends a `pending` row with a pending-color
// status dot + fires a light haptic. The empty feed shows the first-capture
// prompt. Offline shows the queued-sync banner.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/theme/qcue_haptics.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/core/theme/theme_provider.dart';
import 'package:qcue_app/features/capture/capture_screen.dart';
import 'package:qcue_app/widgets/status_dot.dart';

class SpyHaptics implements HapticsSink {
  int light = 0;
  @override
  void lightImpact() => light++;
  @override
  void selectionClick() {}
  @override
  void success() {}
}

Widget _app(StubApiClient api, {SpyHaptics? spy, bool offline = false}) {
  return ProviderScope(
    overrides: [
      apiClientProvider.overrideWithValue(api),
      offlineProvider.overrideWith((ref) => offline),
      if (spy != null) hapticsSinkProvider.overrideWithValue(spy),
    ],
    child: MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: const Scaffold(body: CaptureScreen()),
    ),
  );
}

void main() {
  testWidgets('S4-R30: submitting appends a pending row + fires light haptic',
      (tester) async {
    final spy = SpyHaptics();
    await tester.pumpWidget(_app(StubApiClient.seeded(), spy: spy));
    await tester.pumpAndSettle();

    await tester.enterText(
        find.byKey(const ValueKey('capture-field')), 'a new thought');
    await tester.testTextInput.receiveAction(TextInputAction.send);
    await tester.pumpAndSettle();

    expect(find.text('a new thought'), findsOneWidget);
    // a pending-colored status dot exists for the new row
    final dots = tester
        .widgetList<StatusDot>(find.byType(StatusDot))
        .where((d) => d.state == IngestState.pending);
    expect(dots, isNotEmpty);
    expect(spy.light, 1);
  });

  testWidgets('S4-R3: empty feed renders the first-capture prompt',
      (tester) async {
    await tester.pumpWidget(_app(StubApiClient())); // inert = empty feed
    await tester.pumpAndSettle();
    expect(find.text('Capture your first idea'), findsOneWidget);
  });

  testWidgets('S4-R56: offline shows the queued-sync banner', (tester) async {
    await tester.pumpWidget(_app(StubApiClient.seeded(), offline: true));
    await tester.pumpAndSettle();
    expect(find.textContaining('Offline'), findsOneWidget);
  });

  testWidgets('seeded feed renders captures with status dots', (tester) async {
    await tester.pumpWidget(_app(StubApiClient.seeded()));
    await tester.pumpAndSettle();
    expect(find.byType(StatusDot), findsWidgets);
    expect(
      find.textContaining('Embeddings vs. grep recall'),
      findsOneWidget,
    );
  });
}
