// QCue NG-R12: native "Sign in with Google". Android → OS Credential Manager bottom sheet;
// iOS → GoogleSignIn SDK. Returns the Google ID token (aud = web client id on Android, iOS
// client id on iOS), or null if the user cancels. The backend (POST /v1/auth/social) verifies
// the token against Google's JWKS and the audience allow-list.
import 'package:google_sign_in/google_sign_in.dart';

/// Public web OAuth client ID (not secret). It is the serverClientId on BOTH platforms — the
/// audience the backend verifies on Android. On iOS the iOS client id is read from Info.plist
/// (`GIDClientID`), so it is not needed here.
const String googleWebServerClientId =
    '467938090856-pvomjkhj191bbce5vhn5i9nh9phvb96f.apps.googleusercontent.com';

/// The injectable seam for native Google sign-in. The real impl drives the platform credential
/// UI (can't run under `flutter test`), so [AuthRepository] depends on this interface and tests
/// pass a fake.
abstract interface class GoogleSignInFacade {
  /// Trigger the native account picker and return a Google ID token, or null on cancel.
  Future<String?> signInIdToken();
}

class GoogleNativeSignIn implements GoogleSignInFacade {
  /// The single in-flight/completed initialize() — concurrent callers share it, so the SDK is
  /// initialized exactly once. Reset to null if init fails so a later attempt can retry.
  Future<void>? _init;

  Future<void> _ensureInit() {
    final existing = _init;
    if (existing != null) return existing;
    // serverClientId (web) is the audience the backend verifies; on iOS the iOS client id is
    // read from Info.plist (GIDClientID), so no clientId param is passed here.
    final fut = GoogleSignIn.instance.initialize(serverClientId: googleWebServerClientId);
    _init = fut.catchError((Object e) {
      _init = null; // a failed init is retryable on the next sign-in attempt
      throw e;
    });
    return _init!;
  }

  @override
  Future<String?> signInIdToken() async {
    try {
      await _ensureInit();
      final account = await GoogleSignIn.instance.authenticate(
        scopeHint: const ['email', 'profile'],
      );
      return account.authentication.idToken;
    } on GoogleSignInException catch (e) {
      // A user-cancel is a normal outcome (null → the UI shows "cancelled"); anything else is a
      // real failure the caller should surface.
      if (e.code == GoogleSignInExceptionCode.canceled) return null;
      rethrow;
    }
  }
}
