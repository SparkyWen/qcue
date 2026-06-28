// QCue D4 — the Settings "Voice transcription" provider picker: shows Auto by default and pins a
// configured STT-capable provider (roundtrips through the QcueApiClient seam).
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/settings/settings_screen.dart';

Widget _settingsHost(QcueApiClient api) => ProviderScope(
      overrides: [apiClientProvider.overrideWithValue(api)],
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const Scaffold(body: SettingsScreen()),
      ),
    );

void main() {
  testWidgets('Voice transcription picker shows Auto and pins a provider',
      (tester) async {
    // Settings is a ListView — a tall viewport renders all rows without scrolling.
    tester.view.physicalSize = const Size(1200, 6000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = StubApiClient.seeded();
    await tester.pumpWidget(_settingsHost(api));
    await tester.pumpAndSettle();

    final picker = find.byKey(const ValueKey('stt-provider-picker'));
    expect(picker, findsOneWidget);
    expect(find.descendant(of: picker, matching: find.text('Auto')),
        findsOneWidget,
        reason: 'defaults to Auto');

    // Open the dropdown and pick a configured STT-capable provider.
    await tester.tap(picker);
    await tester.pumpAndSettle();
    await tester.tap(find.text('zhipu').last);
    await tester.pumpAndSettle();

    // The selection roundtrips through the seam (null/auto ⇒ a pinned provider id)...
    expect(api.sttProviderChoice, 'zhipu');
    // ...and the picker re-renders showing the pinned provider after the state refresh.
    expect(find.descendant(of: picker, matching: find.text('zhipu')),
        findsOneWidget);
  });
}
