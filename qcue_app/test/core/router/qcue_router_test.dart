import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:qcue_app/core/router/qcue_router.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/capture/capture_screen.dart';
import 'package:qcue_app/features/onboarding/onboarding_screen.dart';

Widget _app(GoRouter router) => ProviderScope(
      child: MaterialApp.router(
        theme: QCueTheme.build(QThemeId.cleanLight),
        routerConfig: router,
      ),
    );

void main() {
  testWidgets('S4-R7: deep link to a wiki page synthesizes a back-stack', (
    tester,
  ) async {
    final router = buildQcueRouter(initialLocation: '/wiki/page/auto-dream');
    await tester.pumpWidget(_app(router));
    await tester.pumpAndSettle();
    // The real page view renders (seeded stub supplies the page).
    expect(find.text('Auto-Dream'), findsWidgets);
    final ctx = tester.element(find.byType(Navigator).last);
    expect(GoRouter.of(ctx).canPop(), isTrue); // Back → /wiki, not app-exit
  });

  testWidgets('S4-R7: an unknown route renders the typed not-found, not a crash',
      (tester) async {
    final router = buildQcueRouter(initialLocation: '/totally/unknown');
    await tester.pumpWidget(_app(router));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('not-found')), findsOneWidget);
  });

  testWidgets('S4-R52: first run (unauthed, not onboarded) → onboarding',
      (tester) async {
    final router =
        buildQcueRouter(isAuthed: () => false, hasOnboarded: () => false);
    await tester.pumpWidget(_app(router));
    await tester.pumpAndSettle();
    expect(find.byType(OnboardingScreen), findsOneWidget);
  });

  testWidgets('S4-R52: authed bypasses onboarding → app shell', (tester) async {
    final router =
        buildQcueRouter(isAuthed: () => true, hasOnboarded: () => false);
    await tester.pumpWidget(_app(router));
    await tester.pumpAndSettle();
    expect(find.byType(OnboardingScreen), findsNothing);
    expect(find.byType(CaptureScreen), findsOneWidget);
  });

  testWidgets('S4-R6: switching tabs preserves each tab stack', (tester) async {
    final router = buildQcueRouter(initialLocation: '/capture');
    await tester.pumpWidget(_app(router));
    await tester.pumpAndSettle();
    router.go('/wiki/page/auto-dream'); // deep-link into the Wiki branch
    await tester.pumpAndSettle();
    expect(find.text('Auto-Dream'), findsWidgets);

    // Switch away to Recall by tapping its tab (the real switch path:
    // StatefulShellRoute.goBranch, which preserves the inactive branch's stack).
    await tester.tap(find.text('Recall').first);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('recall-empty')), findsOneWidget);

    // Tap back to Wiki — its pushed page is restored (state preservation).
    await tester.tap(find.text('Wiki').first);
    await tester.pumpAndSettle();
    expect(find.text('Auto-Dream'), findsWidgets);
  });
}
