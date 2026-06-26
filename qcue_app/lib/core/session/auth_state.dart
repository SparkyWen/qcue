// QCue: app-global auth state. Promoted to `core/session` (from the auth
// feature) because the ROUTER and the SETTINGS screen both drive it — keeping it
// in `features/auth` forced cross-feature imports (S4-R4). `features/auth` still
// owns the screens + the [AuthRepository]; this is just the shared signed-in
// state + the repository provider the whole app reads.
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../features/auth/auth_repository.dart';
import '../net/api_client_provider.dart';
import '../net/jwt_claims.dart';
import '../offline/offline_api_client.dart';
import '../sync/cache_revision.dart';
import 'session_provider.dart';

/// Whether login is required. `authed` once a (non-empty) session token is held
/// or the demo stub bypasses login; `unauthed` otherwise → router → /login.
enum AuthStatus { unauthed, authed }

class AuthStateNotifier extends Notifier<AuthStatus> {
  @override
  AuthStatus build() {
    // Seed from whatever the token store already holds (a restored session) so
    // a restart resumes signed-in without flashing the login screen.
    final tokens = ref.watch(tokenStoreProvider);
    return tokens.accessSync.isNotEmpty ? AuthStatus.authed : AuthStatus.unauthed;
  }

  /// ISO-R3/R4: wipe the on-device cache and invalidate every user-scoped read-provider. Called on
  /// sign-out so the next account starts from a clean slate. Inert under the keyless stub.
  void _wipeAndReset() {
    final api = ref.read(apiClientProvider);
    if (api is OfflineAwareApiClient) {
      api.cache.clear();
    }
    ref.read(cacheRevisionProvider.notifier).bump();
  }

  /// ISO-R2/R4: claim the cache for the just-signed-in account (wiping it iff a DIFFERENT account
  /// owned it), then invalidate the user-scoped read-providers. Inert under the keyless stub.
  void _adoptOwnerAndReset() {
    final api = ref.read(apiClientProvider);
    final sub = subjectOf(ref.read(tokenStoreProvider).accessSync);
    if (api is OfflineAwareApiClient && sub != null) {
      api.cache.adoptOwner(sub);
    }
    ref.read(cacheRevisionProvider.notifier).bump();
  }

  /// Sign in: persist the session in the [sessionProvider] and flip to authed.
  void signedIn(Session session) {
    ref.read(sessionProvider.notifier).signIn(session);
    _adoptOwnerAndReset(); // ISO-R2/R4
    state = AuthStatus.authed;
  }

  /// Mark the app authenticated without a session object (a restored token or the demo bypass).
  void markAuthed() {
    _adoptOwnerAndReset(); // ISO-R2/R4 (restored token: a no-op wipe when the same account)
    state = AuthStatus.authed;
  }

  /// Mark the app unauthenticated (e.g. a failed refresh-on-401) so the router
  /// redirects to /login. Clears the local session too.
  void markUnauthed() {
    ref.read(sessionProvider.notifier).signOut();
    state = AuthStatus.unauthed;
  }

  /// Sign out: clear tokens (best-effort server revoke) + local session.
  ///
  /// The local token wipe is best-effort: a Keychain/Keystore hiccup must NOT
  /// leave the user wedged in an authed shell — especially after an account
  /// delete, when the server tenant is already gone and every API call would
  /// 401. So even if [logout] throws, we still clear the session and flip to
  /// `unauthed` so the router always bounces to /login.
  Future<void> signOut() async {
    try {
      await ref.read(authRepositoryProvider).logout();
    } catch (_) {
      // swallowed — the session clear + the unauthed flip below are what matter.
    }
    ref.read(sessionProvider.notifier).signOut();
    _wipeAndReset(); // ISO-R3/R4
    state = AuthStatus.unauthed;
  }
}

final authStateProvider =
    NotifierProvider<AuthStateNotifier, AuthStatus>(AuthStateNotifier.new);

/// The auth repository, built from the (bridge-provided) config + token store.
final authRepositoryProvider = Provider<AuthRepository>((ref) {
  return AuthRepository(
    config: ref.watch(qcueConfigProvider),
    tokens: ref.watch(tokenStoreProvider),
  );
});
