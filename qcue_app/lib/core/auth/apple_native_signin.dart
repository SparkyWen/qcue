// QCue SIWA-R1: native "Sign in with Apple". Required by App Store Guideline 4.8 because the app
// also offers "Sign in with Google" — Apple mandates an equivalent privacy-respecting login. iOS
// presents the system Apple ID sheet and returns the Apple identity JWT, or null if the user
// cancels. The backend (POST /v1/auth/social, provider="apple") verifies the token against Apple's
// public keys (https://appleid.apple.com/auth/keys), with aud = the app bundle id — mirroring the
// Google path in google_native_signin.dart.
import 'package:sign_in_with_apple/sign_in_with_apple.dart';

/// The injectable seam for native Apple sign-in. The real impl drives the platform UI (can't run
/// under `flutter test`), so [AuthRepository] depends on this interface and tests pass a fake.
abstract interface class AppleSignInFacade {
  /// Trigger the native Apple ID sheet and return an Apple identity token, or null on cancel.
  Future<String?> signInIdToken();
}

class AppleNativeSignIn implements AppleSignInFacade {
  @override
  Future<String?> signInIdToken() async {
    try {
      final cred = await SignInWithApple.getAppleIDCredential(
        scopes: const [
          AppleIDAuthorizationScopes.email,
          AppleIDAuthorizationScopes.fullName,
        ],
      );
      // The identity token is an Apple-signed JWT; its `sub` + `email` claims are what the backend
      // upserts into oauth_identities. Apple only returns the user's name on the FIRST sign-in, so
      // we rely on the token's claims (email is always present) rather than the credential fields.
      return cred.identityToken;
    } on SignInWithAppleAuthorizationException catch (e) {
      // A user-cancel is a normal outcome (null → the UI shows "cancelled"); anything else is a
      // real failure the caller should surface.
      if (e.code == AuthorizationErrorCode.canceled) return null;
      rethrow;
    }
  }
}
