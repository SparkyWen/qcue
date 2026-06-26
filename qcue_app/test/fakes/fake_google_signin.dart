// QCue NG-R12: a deterministic GoogleSignInFacade for host tests — no native
// Credential Manager / GoogleSignIn SDK, no network. Returns a scripted ID token
// (success) or null (the user cancelled).
import 'package:qcue_app/core/auth/google_native_signin.dart';

class FakeGoogleSignIn implements GoogleSignInFacade {
  FakeGoogleSignIn({this.idToken});

  /// The ID token to hand back; null models a cancel.
  String? idToken;
  int calls = 0;

  @override
  Future<String?> signInIdToken() async {
    calls++;
    return idToken;
  }
}
