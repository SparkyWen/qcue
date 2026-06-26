// QCue S4-R52: onboarding is skippable to a usable keyless app — Intro →
// (continue without an account) → skip key → pick a theme → Finish reaches
// /capture (via onDone), persists the "seen" flag, and leaves the app authed.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/session/auth_state.dart';
import 'package:qcue_app/core/session/session_provider.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/onboarding/onboarding_screen.dart';
import 'package:qcue_app/features/onboarding/onboarding_store.dart';

void main() {
  testWidgets('skip-to-keyless reaches done, persists seen, calls onDone',
      (tester) async {
    final store = InMemoryOnboardingStore();
    var done = false;
    await tester.pumpWidget(ProviderScope(
      overrides: [onboardingStoreProvider.overrideWithValue(store)],
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: OnboardingScreen(onDone: () => done = true),
      ),
    ));

    const primary = ValueKey('onboarding-primary');

    expect(find.byKey(const ValueKey('onboarding-intro')), findsOneWidget);
    await tester.tap(find.byKey(primary)); // Get started
    await tester.pump();
    expect(find.byKey(const ValueKey('onboarding-signin')), findsOneWidget);

    await tester.tap(find.byKey(primary)); // Continue without an account
    await tester.pump();
    expect(find.byKey(const ValueKey('onboarding-addkey')), findsOneWidget);

    final container = ProviderScope.containerOf(
        tester.element(find.byType(OnboardingScreen)));
    expect(container.read(authStateProvider), AuthStatus.authed);
    expect(container.read(sessionProvider), isNull); // keyless: no Session

    await tester.tap(find.byKey(primary)); // Skip key
    await tester.pump();
    expect(find.byKey(const ValueKey('onboarding-theme')), findsOneWidget);

    await tester.tap(find.byKey(ValueKey('theme-${QThemeId.night}')));
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('onboarding-finish')));
    await tester.pump();
    await tester.pump(); // let complete()'s await resolve

    expect(store.hasSeen, isTrue);
    expect(done, isTrue);
  });
}
