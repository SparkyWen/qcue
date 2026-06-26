// QCue S4-R52: the "has the user seen onboarding?" persistence seam. Prod uses
// SharedPreferences (the flag is in the prefs in-memory cache after bootstrap's
// getInstance, so reads are synchronous — the router redirect can decide
// first-run without awaiting). Tests use the in-memory default.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

abstract class OnboardingStore {
  bool get hasSeen;
  Future<void> markSeen();
}

/// In-memory default (tests + the keyless stub): not-seen until marked.
class InMemoryOnboardingStore implements OnboardingStore {
  InMemoryOnboardingStore([this._seen = false]);
  bool _seen;
  @override
  bool get hasSeen => _seen;
  @override
  Future<void> markSeen() async => _seen = true;
}

/// SharedPreferences-backed (prod). `getBool` reads the synchronous in-memory
/// cache populated at `SharedPreferences.getInstance()` (bootstrap).
class SharedPrefsOnboardingStore implements OnboardingStore {
  SharedPrefsOnboardingStore(this._prefs);
  final SharedPreferences _prefs;
  static const _key = 'qcue.onboarding.seen';

  @override
  bool get hasSeen => _prefs.getBool(_key) ?? false;

  @override
  Future<void> markSeen() => _prefs.setBool(_key, true);
}

/// The single onboarding-store seam. Bootstrap overrides it with the
/// SharedPreferences-backed impl (so a genuine first run shows onboarding).
/// The DEFAULT is "already seen" so the onboarding gate is OFF unless a store is
/// configured — that keeps auth-gate tests (unauthed → /login) unaffected;
/// onboarding tests opt in with `InMemoryOnboardingStore(false)`.
final onboardingStoreProvider =
    Provider<OnboardingStore>((_) => InMemoryOnboardingStore(true));
