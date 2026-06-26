// QCue "Sign in with Google": a deterministic [OidcSignIn] for host tests — no
// flutter_appauth / no native ASWebAuthenticationSession, no network. Returns a
// scripted [QcueSession] (success) or null (the user cancelled).
import 'package:qcue_app/core/auth/qcue_oidc.dart';

class FakeOidc implements OidcSignIn {
  FakeOidc({this.session});

  /// The session to hand back from [signInToQcue]; null models a cancel/failure.
  QcueSession? session;

  /// The base URL the screen/repo passed (so a test can assert it used the config).
  String? lastBaseUrl;
  int signInCalls = 0;
  int signOutCalls = 0;

  @override
  Future<QcueSession?> signInToQcue(String qcueBaseUrl) async {
    signInCalls++;
    lastBaseUrl = qcueBaseUrl;
    return session;
  }

  @override
  Future<void> signOut(String? idToken) async {
    signOutCalls++;
  }
}
