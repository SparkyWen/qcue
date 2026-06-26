// QCue App Store / Google Play screenshot harness.
//
// Pumps the real QCueApp with the SAME seeded stub the demo build uses
// (StubApiClient.seeded + a seeded-authed notifier), but WITHOUT main()'s
// ConnectivityHost — so there are no periodic timers and pumpAndSettle is
// deterministic (mirrors test/app_shell_smoke_test.dart, which passes).
//
// Navigates the 4 bottom-bar tabs (Capture / Wiki / Recall / Settings) plus a
// wiki page detail, and captures a full-resolution PNG of each via the driver
// (test_driver/integration_test.dart). Output: qcue_app/screenshots/*.png.
//
// Throwaway-safe: deleting this file + test_driver/integration_test.dart +
// the integration_test dev-dependency removes the harness with no app impact.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'package:qcue_app/app.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/features/auth/auth_provider.dart';

/// A notifier seeded `authed`, so the router never gates the demo behind /login
/// (same trick main.dart uses for the QCUE_STUB demo build).
class _AuthedStateNotifier extends AuthStateNotifier {
  @override
  AuthStatus build() => AuthStatus.authed;
}

List<Override> _stubAuthed() => [
      apiClientProvider.overrideWithValue(StubApiClient.seeded()),
      authStateProvider.overrideWith(() => _AuthedStateNotifier()),
    ];

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('capture store screenshots across the 4 tabs', (tester) async {
    await tester.pumpWidget(
      ProviderScope(overrides: _stubAuthed(), child: const QCueApp()),
    );
    await tester.pumpAndSettle(const Duration(seconds: 2));

    // iOS requires converting the Flutter surface to an image before screenshots.
    await binding.convertFlutterSurfaceToImage();
    await tester.pumpAndSettle();

    Future<void> shoot(String name) async {
      await tester.pumpAndSettle(const Duration(milliseconds: 400));
      await binding.takeScreenshot(name);
    }

    Future<void> openTab(String label) async {
      final tab = find.text(label).first;
      if (tab.evaluate().isEmpty) return;
      await tester.tap(tab);
      await tester.pumpAndSettle(const Duration(seconds: 1));
    }

    // 1) Capture — seeded reverse-chronological feed + the always-ready field.
    await shoot('01-capture');

    // 2) Wiki — the grouped index of generated pages.
    await openTab('Wiki');
    await shoot('02-wiki');

    // 3) Wiki page detail — open a seeded page to showcase the auto-linked wiki
    //    ([[wikilinks]] + backlinks). Try a few known seeded titles by text.
    Finder firstSeededPage = find.text('Recall Architecture');
    if (firstSeededPage.evaluate().isEmpty) firstSeededPage = find.text('Auto-Dream');
    if (firstSeededPage.evaluate().isEmpty) firstSeededPage = find.text('Approvals');
    if (firstSeededPage.evaluate().isNotEmpty) {
      await tester.tap(firstSeededPage.first);
      await tester.pumpAndSettle(const Duration(seconds: 1));
      await shoot('03-wiki-page');
      // Back to the index so the next tab tap is unambiguous.
      final backBtn = find.byTooltip('Back');
      if (backBtn.evaluate().isNotEmpty) {
        await tester.tap(backBtn.first);
        await tester.pumpAndSettle(const Duration(seconds: 1));
      } else {
        await tester.pageBack();
        await tester.pumpAndSettle(const Duration(seconds: 1));
      }
    }

    // 4) Recall — type a sample question so the screen reads intentionally.
    await openTab('Recall');
    final recallInput = find.byKey(const ValueKey('recall-input'));
    if (recallInput.evaluate().isNotEmpty) {
      await tester.enterText(
          recallInput, 'What did I capture about the second-brain idea?');
      await tester.pumpAndSettle(const Duration(milliseconds: 400));
    }
    await shoot('04-recall');

    // 5) Settings — theme, BYOK vault, server, account.
    await openTab('Settings');
    await shoot('05-settings');
  });
}
