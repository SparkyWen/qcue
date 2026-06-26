// QCue cloud-sync fix (Task 5): the email/password auth repository. It talks to
// the app-server's real auth surface (see qcue-rs app-server `auth/routes.rs`):
//   • POST /v1/auth/login    {email, password}      → {access_jwt, refresh_jwt}
//   • POST /v1/auth/refresh  {refresh_token}        → {access_jwt, refresh_jwt}
//   • POST /v1/auth/logout   {refresh_token}        → {ok: true}
//
// On success it persists the JWT pair into the durable [TokenStore]
// ([SecureTokenStore] in production) so the bearer is non-empty on every
// subsequent capture POST — the fix for the empty-`Bearer ` 401 that silently
// kept notes queued forever.
//
// Failures are TYPED so the UI can tell the user WHY: [AuthError.invalidCredentials]
// (401 from the server) vs [AuthError.network] (server unreachable / wrong URL).
import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;

import '../../core/auth/apple_native_signin.dart'; // native "Sign in with Apple" seam (SIWA-R1)
import '../../core/auth/google_native_signin.dart'; // native "Sign in with Google" seam
import '../../core/net/api_client_provider.dart'; // bridge: QcueConfig, TokenStore (S4-R1 seam)
import '../../core/session/session_provider.dart';

/// Why an auth call failed — surfaced to the login/signup screen for a precise
/// message.
enum AuthErrorKind { invalidCredentials, emailTaken, network, server }

class AuthError implements Exception {
  AuthError(this.kind, [this.message = '']);
  final AuthErrorKind kind;
  final String message;

  /// 401 from the server: the email/password pair was rejected.
  static AuthError invalidCredentials([String m = '']) =>
      AuthError(AuthErrorKind.invalidCredentials, m);

  /// 400 from signup: the email is already registered to another account.
  static AuthError emailTaken([String m = '']) =>
      AuthError(AuthErrorKind.emailTaken, m);

  /// The server could not be reached at all (DNS, refused, timeout) — usually a
  /// wrong/unset Server URL or an offline device.
  static AuthError network([String m = '']) => AuthError(AuthErrorKind.network, m);

  /// A non-401 server error (5xx / unexpected body).
  static AuthError server([String m = '']) => AuthError(AuthErrorKind.server, m);

  @override
  String toString() => 'AuthError(${kind.name}${message.isEmpty ? '' : ': $message'})';
}

/// Talks to the backend auth routes and persists the session JWT pair.
class AuthRepository {
  AuthRepository({
    required QcueConfig config,
    required TokenStore tokens,
    http.Client? httpClient,
    GoogleSignInFacade? google,
    AppleSignInFacade? apple,
  })  :
        // ignore: prefer_initializing_formals
        _config = config,
        // ignore: prefer_initializing_formals
        _tokens = tokens,
        _http = httpClient ?? http.Client(),
        _google = google ?? GoogleNativeSignIn(),
        _apple = apple ?? AppleNativeSignIn();

  final QcueConfig _config;
  final TokenStore _tokens;
  final http.Client _http;
  final GoogleSignInFacade _google;
  final AppleSignInFacade _apple;

  /// AUTH-R3: the in-flight refresh. While set, all callers of [refresh] await
  /// the SAME future so exactly one `/v1/auth/refresh` runs for N concurrent 401s.
  Future<bool>? _inflightRefresh;

  /// POST /v1/auth/login. On success stores access+refresh and returns the
  /// [Session]. Throws [AuthError] (invalidCredentials | network | server).
  Future<Session> login(String email, String password) async {
    final body = await _post('/v1/auth/login', {
      'email': email,
      'password': password,
    });
    final access = body['access_jwt'] as String?;
    final refresh = body['refresh_jwt'] as String? ?? '';
    if (access == null || access.isEmpty) {
      throw AuthError.server('login response missing access_jwt');
    }
    await _tokens.write(access: access, refresh: refresh);
    await _persistExpiry(body['expires_at']);
    return Session(jwt: access, email: email, hasKey: false);
  }

  /// POST /v1/auth/signup. Creates the account and, on success, stores
  /// access+refresh and returns the [Session] EXACTLY like [login]. A duplicate
  /// email comes back as a 400 ("email already registered") → mapped to
  /// [AuthError.emailTaken]; empty fields 401 → [AuthError.invalidCredentials];
  /// network/5xx behave like [login].
  Future<Session> signup(String email, String password) async {
    final body = await _post('/v1/auth/signup', {
      'email': email,
      'password': password,
    }, surfaceBadRequest: true);
    final access = body['access_jwt'] as String?;
    final refresh = body['refresh_jwt'] as String? ?? '';
    if (access == null || access.isEmpty) {
      throw AuthError.server('signup response missing access_jwt');
    }
    await _tokens.write(access: access, refresh: refresh);
    await _persistExpiry(body['expires_at']);
    return Session(jwt: access, email: email, hasKey: false);
  }

  /// "Sign in with Google" → a native qcue session. Triggers the OS account picker
  /// (Android Credential Manager bottom sheet / iOS GoogleSignIn), gets a Google ID
  /// token, exchanges it at `POST /v1/auth/social`, then persists the returned
  /// access+refresh pair + expiry EXACTLY like [login]/[signup] — so every subsequent
  /// API call carries the bearer and the proactive-refresh timer (AUTH-R5) is scheduled.
  /// Returns the [Session] on success, or null if the user cancelled (the caller shows a
  /// precise "cancelled" message). The qcue access JWT carries no email claim, so
  /// [Session.email] is empty for a Google session.
  Future<Session?> loginWithGoogle() async {
    final idToken = await _google.signInIdToken();
    if (idToken == null || idToken.isEmpty) return null;
    final body = await _post('/v1/auth/social', {
      'provider': 'google',
      'id_token': idToken,
    });
    final access = body['access_jwt'] as String?;
    final refresh = body['refresh_jwt'] as String? ?? '';
    if (access == null || access.isEmpty) {
      throw AuthError.server('social response missing access_jwt');
    }
    await _tokens.write(access: access, refresh: refresh);
    await _persistExpiry(body['expires_at']);
    return Session(jwt: access, email: '', hasKey: false);
  }

  /// "Sign in with Apple" (SIWA-R1) → a native qcue session. Presents the system Apple ID sheet,
  /// gets an Apple identity JWT, exchanges it at `POST /v1/auth/social` with provider="apple", then
  /// persists the returned access+refresh pair + expiry EXACTLY like [loginWithGoogle]. Returns the
  /// [Session] on success, or null if the user cancelled. The qcue access JWT carries no email
  /// claim, so [Session.email] is empty for an Apple session.
  Future<Session?> loginWithApple() async {
    final idToken = await _apple.signInIdToken();
    if (idToken == null || idToken.isEmpty) return null;
    final body = await _post('/v1/auth/social', {
      'provider': 'apple',
      'id_token': idToken,
    });
    final access = body['access_jwt'] as String?;
    final refresh = body['refresh_jwt'] as String? ?? '';
    if (access == null || access.isEmpty) {
      throw AuthError.server('social response missing access_jwt');
    }
    await _tokens.write(access: access, refresh: refresh);
    await _persistExpiry(body['expires_at']);
    return Session(jwt: access, email: '', hasKey: false);
  }

  /// POST /v1/auth/refresh {refresh_token}. SINGLE-FLIGHT (AUTH-R3): concurrent
  /// callers share one in-flight future, so N simultaneous 401s trigger exactly
  /// one rotation. Returns true on success; false if there is no refresh token or
  /// the server rejects it (caller should route to /login). A network/transport
  /// failure returns false WITHOUT clearing the pair (AUTH-R6).
  Future<bool> refresh() {
    final existing = _inflightRefresh;
    if (existing != null) return existing;
    final fut = _refreshOnce().whenComplete(() => _inflightRefresh = null);
    _inflightRefresh = fut;
    return fut;
  }

  Future<bool> _refreshOnce() async {
    final refreshToken = await _tokens.readRefresh();
    if (refreshToken == null || refreshToken.isEmpty) return false;
    try {
      final body = await _post('/v1/auth/refresh', {
        'refresh_token': refreshToken,
      });
      final access = body['access_jwt'] as String?;
      final newRefresh = body['refresh_jwt'] as String? ?? refreshToken;
      if (access == null || access.isEmpty) return false;
      await _tokens.write(access: access, refresh: newRefresh);
      await _persistExpiry(body['expires_at']);
      return true;
    } on AuthError catch (e) {
      // AUTH-R6: ONLY a definitive 401 (the refresh token is genuinely dead)
      // signs out locally. A network/transport error leaves the pair intact so a
      // later attempt can succeed once connectivity returns.
      if (e.kind == AuthErrorKind.invalidCredentials) {
        await _tokens.clear();
      }
      return false;
    } catch (_) {
      // AUTH-R3/R6: any OTHER failure (a malformed 200 body → FormatException, a
      // secure-store read hiccup, …) must still resolve the single-flight future
      // to `false` — never let it reject, or concurrent awaiters would throw
      // instead of getting a bool. A non-401 failure does NOT clear the pair.
      return false;
    }
  }

  /// Persist the access expiry (AUTH-R5) if the server returned one. Tolerates a
  /// missing/garbage value (older server) — the proactive timer just falls back.
  /// Best-effort: a secure-store write hiccup must not fail an otherwise
  /// successful refresh (the new token pair is already persisted).
  Future<void> _persistExpiry(Object? raw) async {
    if (raw is! String || raw.isEmpty) return;
    final exp = DateTime.tryParse(raw);
    if (exp == null) return;
    try {
      await _tokens.writeExpiry(exp.toUtc());
    } catch (_) {
      // ignore: expiry persistence is advisory, the refresh already succeeded.
    }
  }

  /// POST /v1/auth/logout (best-effort) then clear the local token pair.
  Future<void> logout() async {
    final refreshToken = await _tokens.readRefresh();
    if (refreshToken != null && refreshToken.isNotEmpty) {
      try {
        await _post('/v1/auth/logout', {'refresh_token': refreshToken});
      } catch (_) {
        // logout is best-effort; the local clear below is what matters.
      }
    }
    await _tokens.clear();
  }

  /// Whether a (non-empty) access token is currently held.
  bool get hasSession => _tokens.accessSync.isNotEmpty;

  Future<Map<String, dynamic>> _post(
      String path, Map<String, dynamic> body,
      {bool surfaceBadRequest = false}) async {
    final http.Response r;
    try {
      r = await _http.post(
        _config.uri(path),
        headers: const {
          'Content-Type': 'application/json',
          'Accept': 'application/json',
        },
        body: jsonEncode(body),
      );
    } catch (e) {
      // A transport failure (connection refused, DNS, timeout) — distinct from a
      // 401, so the UI can say "can't reach server / check Server URL".
      throw AuthError.network(e.toString());
    }
    if (r.statusCode == 401) {
      throw AuthError.invalidCredentials();
    }
    // Signup's "email already registered" comes back as a 400 with a body
    // message — surface it so the UI can tell the user the email is taken.
    // utf8.decode(bodyBytes) (not latin-1 r.body) — the server's application/json carries no charset,
    // so a localized message (e.g. "该邮箱已被注册") must be read as UTF-8 or it surfaces as mojibake.
    if (surfaceBadRequest && r.statusCode == 400) {
      throw AuthError.emailTaken(
          _messageOf(utf8.decode(r.bodyBytes), 'email already registered'));
    }
    if (r.statusCode < 200 || r.statusCode >= 300) {
      throw AuthError.server('HTTP ${r.statusCode}');
    }
    if (r.bodyBytes.isEmpty) return const {};
    final decoded = jsonDecode(utf8.decode(r.bodyBytes));
    return decoded is Map ? decoded.cast<String, dynamic>() : const {};
  }

  /// Pull a human message out of an error body (`{"message": "..."}`/`{"error":
  /// "..."}`), falling back to [fallback] when the body has none.
  static String _messageOf(String rawBody, String fallback) {
    if (rawBody.isEmpty) return fallback;
    try {
      final decoded = jsonDecode(rawBody);
      if (decoded is Map) {
        final m = decoded['message'] ?? decoded['error'];
        if (m is String && m.isNotEmpty) return m;
      }
    } catch (_) {
      // Non-JSON body: use it verbatim if it looks like a message.
      final t = rawBody.trim();
      if (t.isNotEmpty) return t;
    }
    return fallback;
  }

  void dispose() => _http.close();
}
