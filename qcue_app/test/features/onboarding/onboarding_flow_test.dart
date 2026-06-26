// QCue S4-R52/R53: the onboarding controller — step navigation, the keyless
// skip (a usable app with no account), the persisted "seen" flag, and an
// idempotent auth-callback resume (no double session).
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/session/auth_state.dart';
import 'package:qcue_app/core/session/session_provider.dart';
import 'package:qcue_app/features/onboarding/onboarding_flow.dart';
import 'package:qcue_app/features/onboarding/onboarding_store.dart';

void main() {
  late ProviderContainer c;
  late InMemoryOnboardingStore store;

  setUp(() {
    store = InMemoryOnboardingStore();
    c = ProviderContainer(
      overrides: [onboardingStoreProvider.overrideWithValue(store)],
    );
  });
  tearDown(() => c.dispose());

  OnboardingController ctrl() => c.read(onboardingControllerProvider.notifier);
  OnboardingStep step() => c.read(onboardingControllerProvider);

  test('steps advance intro → signIn → addKey → theme → done', () {
    expect(step(), OnboardingStep.intro);
    ctrl().next();
    expect(step(), OnboardingStep.signIn);
    ctrl()
      ..next()
      ..next()
      ..next();
    expect(step(), OnboardingStep.done);
  });

  test('continueKeyless lands a usable keyless app, no session (S4-R52)', () {
    ctrl().continueKeyless();
    expect(c.read(authStateProvider), AuthStatus.authed);
    expect(c.read(sessionProvider), isNull); // keyless: authed, no Session/JWT
    expect(step(), OnboardingStep.addKey);
  });

  test('complete persists the seen flag', () async {
    await ctrl().complete();
    expect(store.hasSeen, isTrue);
    expect(step(), OnboardingStep.done);
  });

  test('signedIn records exactly one session — idempotent (S4-R53)', () {
    // NOTE: the prior test drove the removed `handleAuthCallback` deep-link path (it trusted a
    // `?token=` JWT verbatim — a session-fixation footgun, now deleted). Idempotency of session
    // creation is now asserted directly on `signedIn`, the real (verified) sign-in entry point.
    const session = Session(jwt: 'jwt-1', email: 'a@b.com', hasKey: false);
    ctrl().signedIn(session);
    ctrl().signedIn(session); // a duplicate sign-in must not bump the counter
    expect(ctrl().sessionsCreated, 1);
    expect(c.read(authStateProvider), AuthStatus.authed);
    expect(c.read(sessionProvider)?.email, 'a@b.com');
  });
}
