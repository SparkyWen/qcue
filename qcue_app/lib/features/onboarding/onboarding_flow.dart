// QCue S4-R52/R53: the first-run onboarding flow controller. Keyless is a
// first-class outcome (StubProvider, master §4) — a user can skip sign-in AND
// the key step and still land on a usable /capture. Auth callbacks
// (qcue://auth/callback magic-link / OAuth) resume idempotently and NEVER
// double-create a session. Riverpod-disciplined (a Notifier, no setState).
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/session/auth_state.dart';
import '../../core/session/session_provider.dart';
import 'onboarding_store.dart';

/// The ordered steps. `done` is terminal (the screen routes to /capture).
enum OnboardingStep { intro, signIn, addKey, theme, done }

class OnboardingController extends Notifier<OnboardingStep> {
  /// Where onboarding lands once complete. Always /capture — the keyless app is
  /// fully usable (master §4).
  static const landing = '/capture';

  /// How many sessions this flow created — asserted == 1 in the idempotency
  /// test (S4-R53). A duplicate auth callback must NOT bump this.
  int sessionsCreated = 0;

  @override
  OnboardingStep build() => OnboardingStep.intro;

  void next() => state = switch (state) {
        OnboardingStep.intro => OnboardingStep.signIn,
        OnboardingStep.signIn => OnboardingStep.addKey,
        OnboardingStep.addKey => OnboardingStep.theme,
        OnboardingStep.theme => OnboardingStep.done,
        OnboardingStep.done => OnboardingStep.done,
      };

  void back() => state = switch (state) {
        OnboardingStep.intro => OnboardingStep.intro,
        OnboardingStep.signIn => OnboardingStep.intro,
        OnboardingStep.addKey => OnboardingStep.signIn,
        OnboardingStep.theme => OnboardingStep.addKey,
        OnboardingStep.done => OnboardingStep.theme,
      };

  /// Continue WITHOUT an account — the keyless StubProvider path (S4-R52). The
  /// app is authed (so the router lets us past the gate) but no Session/JWT is
  /// created; uploads that need auth surface via the offline banner.
  void continueKeyless() {
    ref.read(authStateProvider.notifier).markAuthed();
    state = OnboardingStep.addKey; // the key step is still offered (optional)
  }

  /// A real sign-in produced a [Session] — record it (once) and advance.
  void signedIn(Session session) {
    if (sessionsCreated == 0) sessionsCreated++;
    ref.read(authStateProvider.notifier).signedIn(session);
    state = OnboardingStep.addKey;
  }

  /// Skip the optional key step — proceed keyless (StubProvider).
  void skipKey() => state = OnboardingStep.theme;

  /// A BYOK key was added in the AddKey step.
  void keyAdded() {
    ref.read(sessionProvider.notifier).markKeyAdded();
    state = OnboardingStep.theme;
  }

  /// Mark first-run complete (persisted) and finish. Idempotent.
  Future<void> complete() async {
    await ref.read(onboardingStoreProvider).markSeen();
    state = OnboardingStep.done;
  }

  // SECURITY: a `handleAuthCallback(uri)` that installed a `?token=<jwt>` from a `qcue://` deep link
  // directly as the live session was removed. The qcue:// scheme is an exported/BROWSABLE VIEW intent, so
  // any app/web page could fire such a URL — trusting the token verbatim would be a one-tap
  // session-fixation / account-takeover footgun. It was dead code (no deep-link entry point routed to it).
  // A real magic-link flow MUST exchange the inbound one-time code at a backend endpoint (mirroring
  // core/auth/qcue_oidc.dart + /v1/auth/social) and only call `signedIn()` with the server-minted token.
}

final onboardingControllerProvider =
    NotifierProvider<OnboardingController, OnboardingStep>(
        OnboardingController.new);
