// "Sign in with QCue" via the auth.qcue.cn OIDC provider. The hosted login page offers Google / passkey /
// password; this just launches the Authorization-Code + PKCE flow and returns tokens.
//
// Requires (see auth repo docs/google-login-setup.md):
//   pubspec.yaml:  flutter_appauth: ^12.0.0   (and flutter_secure_storage for persistence)
//   Redirect: the VERIFIED https App Link `https://app.qcue.cn/applink/oauth2redirect` (Android binds it
//   to the signed app via assetlinks.json + the autoVerify intent-filter; see qcue_app/android/APP_LINKS.md).
//   The legacy `qcue://` filter is kept during the transition until on-device verification passes.
//
// API note: flutter_appauth's method shapes vary by major version. This targets ^12.x (the
// `externalUserAgent` enum below landed in 8.0.0 and is unchanged through 12.x); on cancel it
// throws (we catch and return null).
import 'dart:convert';
import 'dart:io';

import 'package:flutter_appauth/flutter_appauth.dart';

/// The qcue backend session returned by the OIDC bridge (the same shape as email/password login).
class QcueSession {
  final String accessJwt;
  final String refreshJwt;

  /// The access token's expiry as the backend's raw ISO-8601 string (`expires_at`), or null if
  /// the bridge didn't return one. Persisted like email login so the proactive-refresh timer
  /// (AUTH-R5) schedules a rotation at ~80% of TTL for Google sessions too.
  final String? expiresAt;
  const QcueSession({required this.accessJwt, required this.refreshJwt, this.expiresAt});
}

/// The injectable seam for the "Sign in with Google → qcue session" flow. The real [QcueOidc]
/// drives flutter_appauth's native ASWebAuthenticationSession, which can't run under `flutter test`,
/// so the login screen + [AuthRepository] depend on this interface and tests pass a fake.
abstract interface class OidcSignIn {
  /// FULL flow: hosted OIDC login (the page offers Google) → exchange at the qcue backend for a
  /// native session. Returns null if the user cancels or the exchange fails.
  Future<QcueSession?> signInToQcue(String qcueBaseUrl);

  /// RP-initiated logout at the provider (best-effort; safe to call with a null id token).
  Future<void> signOut(String? idToken);
}

class QcueTokens {
  final String accessToken;
  final String? idToken;
  final String? refreshToken;
  final DateTime? accessTokenExpiry;
  const QcueTokens({required this.accessToken, this.idToken, this.refreshToken, this.accessTokenExpiry});
}

class QcueOidc implements OidcSignIn {
  static const String issuer = 'https://auth.qcue.cn';
  static const String clientId = 'qcue-ios';

  /// SECURITY (App Links hardening): a VERIFIED https App Link, not the custom `qcue://` scheme. Any
  /// installed app can register `qcue://`, so the old `qcue://oauth2redirect` callback was hijackable
  /// (PKCE blocked token theft, but not callback-stealing/phishing). Android binds this https link
  /// exclusively to the signed app via /.well-known/assetlinks.json + the autoVerify intent-filter in
  /// AndroidManifest.xml (path-scoped to /applink). iOS uses the same URL via Universal Links / the
  /// Info.plist scheme. The Google OAuth client must list this exact redirect URI. See
  /// qcue_app/android/APP_LINKS.md.
  static const String redirectUrl = 'https://app.qcue.cn/applink/oauth2redirect';
  static const List<String> scopes = ['openid', 'profile', 'email', 'offline_access'];

  final FlutterAppAuth _appAuth = const FlutterAppAuth();

  /// Launch the hosted login (where the user can tap "Sign in with Google"), then exchange the code for tokens.
  /// Returns null if the user cancels.
  Future<QcueTokens?> signIn() async {
    try {
      final r = await _appAuth.authorizeAndExchangeCode(
        AuthorizationTokenRequest(
          clientId,
          redirectUrl,
          issuer: issuer,
          scopes: scopes,
          promptValues: const ['login'],
          // flutter_appauth 8.x: the old `preferEphemeralSession: true` is now an enum. The
          // ephemeral variant keeps the web session isolated (no shared cookies / cache) on iOS.
          externalUserAgent: ExternalUserAgent.ephemeralAsWebAuthenticationSession,
        ),
      );
      if (r.accessToken == null) return null;
      return QcueTokens(
        accessToken: r.accessToken!,
        idToken: r.idToken,
        refreshToken: r.refreshToken,
        accessTokenExpiry: r.accessTokenExpirationDateTime,
      );
    } catch (_) {
      return null; // user cancelled / dismissed
    }
  }

  /// Refresh the access token from a stored refresh token.
  Future<QcueTokens?> refresh(String refreshToken) async {
    final r = await _appAuth.token(
      TokenRequest(clientId, redirectUrl, issuer: issuer, refreshToken: refreshToken, scopes: scopes),
    );
    if (r.accessToken == null) return null;
    return QcueTokens(
      accessToken: r.accessToken!,
      idToken: r.idToken,
      refreshToken: r.refreshToken ?? refreshToken,
      accessTokenExpiry: r.accessTokenExpirationDateTime,
    );
  }

  /// FULL "Sign in with Google → into qcue" flow:
  ///   1) OIDC login via auth.qcue.cn (the hosted page offers Google) → an auth.qcue.cn access_token;
  ///   2) exchange it at the qcue backend POST {qcueBaseUrl}/v1/auth/oidc → a NATIVE qcue session.
  /// Store the returned {accessJwt, refreshJwt} exactly like the email/password login does. Returns null on
  /// cancel or failure. `qcueBaseUrl` is your qcue backend base, e.g. https://app.qcue.cn (no trailing slash).
  @override
  Future<QcueSession?> signInToQcue(String qcueBaseUrl) async {
    final tokens = await signIn();
    if (tokens == null) return null;
    final client = HttpClient();
    try {
      final r = await client.postUrl(Uri.parse('$qcueBaseUrl/v1/auth/oidc'));
      r.headers.contentType = ContentType.json;
      r.write(jsonEncode({'access_token': tokens.accessToken}));
      final resp = await r.close();
      if (resp.statusCode != 200) return null;
      final body = jsonDecode(await resp.transform(utf8.decoder).join()) as Map<String, dynamic>;
      final access = body['access_jwt'] as String?;
      final refresh = body['refresh_jwt'] as String?;
      if (access == null || refresh == null) return null;
      return QcueSession(
        accessJwt: access,
        refreshJwt: refresh,
        expiresAt: body['expires_at'] as String?,
      );
    } catch (_) {
      return null;
    } finally {
      client.close();
    }
  }

  /// RP-initiated logout at the provider.
  @override
  Future<void> signOut(String? idToken) async {
    try {
      await _appAuth.endSession(
        EndSessionRequest(idTokenHint: idToken, postLogoutRedirectUrl: redirectUrl, issuer: issuer),
      );
    } catch (_) {}
  }
}
