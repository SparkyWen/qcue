// QCue SIWA-R1: a deterministic AppleSignInFacade for host tests — no native Apple ID sheet, no
// network. Returns a scripted identity token (success) or null (the user cancelled). Mirrors
// FakeGoogleSignIn.
import 'package:qcue_app/core/auth/apple_native_signin.dart';

class FakeAppleSignIn implements AppleSignInFacade {
  FakeAppleSignIn({this.idToken});

  /// The identity token to hand back; null models a cancel.
  String? idToken;
  int calls = 0;

  @override
  Future<String?> signInIdToken() async {
    calls++;
    return idToken;
  }
}
