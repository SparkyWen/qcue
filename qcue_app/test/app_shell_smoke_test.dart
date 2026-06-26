// QCue S4 shell smoke test: pumps the whole app and asserts the 4-tab IA, that
// tab navigation works, and that the live theme switcher actually re-themes.
// (v0.2.2: Activity moved off the bottom bar into Settings.)
//
// Cloud-sync fix (Task 5): the app now gates behind /login when unauthenticated,
// so the shell tests seed an in-memory access token — a signed-in session — to
// exercise the post-login shell (the auth gate has its own tests).
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/app.dart';
import 'package:qcue_app/core/net/qcue_config.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/features/auth/auth_provider.dart';

/// A signed-in scope: a non-empty token → the router stays on the app shell.
List<Override> _authed() => [
      tokenStoreProvider
          .overrideWithValue(InMemoryTokenStore(access: 'test-access')),
    ];

void main() {
  testWidgets('the app boots into a 4-tab Clean Light shell', (tester) async {
    await tester.pumpWidget(
        ProviderScope(overrides: _authed(), child: const QCueApp()));
    await tester.pumpAndSettle();

    // The 4 bottom-bar tabs are present (Activity moved into Settings, v0.2.2).
    for (final label in ['Capture', 'Wiki', 'Recall', 'Settings']) {
      expect(find.text(label), findsWidgets);
    }
    expect(find.text('Activity'), findsNothing);
    // Capture is the default tab (its always-ready field is pinned at bottom).
    expect(find.byKey(const ValueKey('capture-field')), findsOneWidget);
  });

  testWidgets('tapping a tab navigates and preserves the shell', (tester) async {
    await tester.pumpWidget(
        ProviderScope(overrides: _authed(), child: const QCueApp()));
    await tester.pumpAndSettle();

    await tester.tap(find.text('Wiki').first);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('wiki-search')), findsOneWidget);

    await tester.tap(find.text('Settings').first);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('settings-screen')), findsOneWidget);
  });

  testWidgets('the Settings theme switcher re-themes the app live', (
    tester,
  ) async {
    await tester.pumpWidget(
        ProviderScope(overrides: _authed(), child: const QCueApp()));
    await tester.pumpAndSettle();

    await tester.tap(find.text('Settings').first);
    await tester.pumpAndSettle();

    // Default scaffold bg is Clean Light's bg.
    BuildContext ctx = tester.element(find.byType(Scaffold).first);
    expect(Theme.of(ctx).extension<QCueColors>()!.bg.toARGB32(),
        0xFFFFFFFF);

    // Select Night and confirm the active theme changed.
    await tester.tap(find.text('Night'));
    await tester.pumpAndSettle();
    ctx = tester.element(find.byType(Scaffold).first);
    expect(Theme.of(ctx).extension<QCueColors>()!.bg.toARGB32(),
        0xFF191919);
  });
}
