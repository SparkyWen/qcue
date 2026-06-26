// QCue cloud-sync fix (Task 5): the email/password auth feature. Exercised
// against an in-process app-server stand-in (no real server / no network), the
// SAME pattern as http_api_client_test. Pins:
//   - login success stores the access+refresh pair + returns the session;
//   - a 401 surfaces AuthError.invalidCredentials (→ "wrong email/password");
//   - an unreachable server surfaces AuthError.network (→ "check Server URL");
//   - refresh() rotates the pair and persists the new tokens;
//   - HttpApiClient retries ONCE after a refresh-on-401, then succeeds.
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/http_api_client.dart';
import 'package:qcue_app/core/net/qcue_config.dart';
import 'package:qcue_app/features/auth/auth_repository.dart';

import '../../fakes/fake_apple_signin.dart';
import '../../fakes/fake_google_signin.dart';

/// A scriptable auth/server stand-in. Mints distinct tokens per call so a
/// rotation is observable; can be told to 401 the login and to 401 the first
/// authed call (to exercise refresh-on-401).
class FakeAuthServer {
  late final HttpServer _server;
  bool rejectLogin = false; // login → 401
  int authedCallsToReject = 0; // first N authed calls → 401 (then succeed)
  int refreshCount = 0;
  int issued = 0;

  Future<void> start() async {
    _server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    _server.listen(_handle);
  }

  String get base => 'http://127.0.0.1:${_server.port}';
  Future<void> stop() => _server.close(force: true);

  Map<String, dynamic> _pair() {
    issued++;
    return {
      'access_jwt': 'access-$issued',
      'refresh_jwt': 'refresh-$issued',
      'expires_at': '2026-06-14T10:30:00Z',
    };
  }

  Future<void> _handle(HttpRequest req) async {
    final path = req.uri.path;
    final res = req.response;
    Future<void> json(Object body, {int status = 200}) async {
      res.statusCode = status;
      res.headers.contentType = ContentType.json;
      res.write(jsonEncode(body));
      await res.close();
    }

    if (path == '/v1/auth/login' && req.method == 'POST') {
      final body = jsonDecode(await utf8.decoder.bind(req).join())
          as Map<String, dynamic>;
      expect(body.keys.toSet(), {'email', 'password'});
      if (rejectLogin) return json({'error': 'bad creds'}, status: 401);
      return json(_pair());
    }
    if (path == '/v1/auth/refresh' && req.method == 'POST') {
      final body = jsonDecode(await utf8.decoder.bind(req).join())
          as Map<String, dynamic>;
      // The server field is `refresh_token` (NOT refresh_jwt) — pin it.
      expect(body.keys.toSet(), {'refresh_token'});
      refreshCount++;
      return json(_pair());
    }
    if (path == '/v1/auth/social' && req.method == 'POST') {
      final body = jsonDecode(await utf8.decoder.bind(req).join())
          as Map<String, dynamic>;
      expect(body.keys.toSet(), {'provider', 'id_token'});
      expect(body['provider'], anyOf('google', 'apple'));
      if ((body['id_token'] as String).isEmpty) {
        return json({'error': 'bad token'}, status: 401);
      }
      return json(_pair());
    }
    if (path == '/v1/captures' && req.method == 'GET') {
      if (authedCallsToReject > 0) {
        authedCallsToReject--;
        return json({'error': {'code': -32002, 'message': 'unauthorized'}},
            status: 401);
      }
      return json({'captures': []});
    }
    res.statusCode = 404;
    await res.close();
  }
}

QcueConfig _cfg(String base) => QcueConfig(baseUrl: base);

void main() {
  late FakeAuthServer server;

  setUp(() async {
    server = FakeAuthServer();
    await server.start();
  });
  tearDown(() => server.stop());

  test('login success stores the access+refresh pair and returns the session',
      () async {
    final tokens = InMemoryTokenStore();
    final repo = AuthRepository(config: _cfg(server.base), tokens: tokens);

    final session = await repo.login('a@b.co', 'pw');

    expect(session.email, 'a@b.co');
    expect(session.jwt, 'access-1');
    expect(tokens.accessSync, 'access-1');
    expect(await tokens.readRefresh(), 'refresh-1');
    repo.dispose();
  });

  test('a 401 from login surfaces AuthError.invalidCredentials', () async {
    server.rejectLogin = true;
    final repo = AuthRepository(
        config: _cfg(server.base), tokens: InMemoryTokenStore());
    await expectLater(
      repo.login('a@b.co', 'wrong'),
      throwsA(isA<AuthError>().having(
          (e) => e.kind, 'kind', AuthErrorKind.invalidCredentials)),
    );
    repo.dispose();
  });

  test('an unreachable server surfaces AuthError.network', () async {
    // Point at a closed port (server started then stopped) → connection refused.
    final base = server.base;
    await server.stop();
    final repo =
        AuthRepository(config: _cfg(base), tokens: InMemoryTokenStore());
    await expectLater(
      repo.login('a@b.co', 'pw'),
      throwsA(isA<AuthError>()
          .having((e) => e.kind, 'kind', AuthErrorKind.network)),
    );
    repo.dispose();
  });

  test('refresh() rotates the pair and persists the new tokens', () async {
    final tokens = InMemoryTokenStore(access: 'old', refresh: 'refresh-old');
    final repo = AuthRepository(config: _cfg(server.base), tokens: tokens);

    final ok = await repo.refresh();
    expect(ok, isTrue);
    expect(server.refreshCount, 1);
    expect(tokens.accessSync, 'access-1'); // rotated
    expect(await tokens.readRefresh(), 'refresh-1');
    repo.dispose();
  });

  test('refresh() returns false when there is no refresh token', () async {
    final repo = AuthRepository(
        config: _cfg(server.base), tokens: InMemoryTokenStore());
    expect(await repo.refresh(), isFalse);
    expect(server.refreshCount, 0);
    repo.dispose();
  });

  test('logout clears the local token pair', () async {
    final tokens = InMemoryTokenStore(access: 'a', refresh: 'r');
    final repo = AuthRepository(config: _cfg(server.base), tokens: tokens);
    await repo.logout();
    expect(tokens.accessSync, isEmpty);
    expect(await tokens.readRefresh(), isNull);
    repo.dispose();
  });

  test('loginWithGoogle posts the native id_token and stores the session',
      () async {
    final tokens = InMemoryTokenStore();
    final google = FakeGoogleSignIn(idToken: 'goog-id-token');
    final repo = AuthRepository(
        config: _cfg(server.base), tokens: tokens, google: google);

    final session = await repo.loginWithGoogle();

    expect(session, isNotNull);
    expect(google.calls, 1);
    expect(session!.jwt, 'access-1'); // server minted via _pair()
    expect(tokens.accessSync, 'access-1');
    expect(await tokens.readRefresh(), 'refresh-1');
    // The server's expiry is persisted so the proactive-refresh timer (AUTH-R5) works.
    expect(tokens.expiresAtSync, DateTime.utc(2026, 6, 14, 10, 30));
    repo.dispose();
  });

  test('loginWithGoogle returns null and writes nothing when cancelled',
      () async {
    final tokens = InMemoryTokenStore();
    final repo = AuthRepository(
        config: _cfg(server.base),
        tokens: tokens,
        google: FakeGoogleSignIn(idToken: null));

    final session = await repo.loginWithGoogle();

    expect(session, isNull);
    expect(tokens.accessSync, isEmpty);
    expect(await tokens.readRefresh(), isNull);
    repo.dispose();
  });

  test('loginWithApple posts the apple id_token and stores the session',
      () async {
    final tokens = InMemoryTokenStore();
    final apple = FakeAppleSignIn(idToken: 'apple-id-token');
    final repo = AuthRepository(
        config: _cfg(server.base), tokens: tokens, apple: apple);

    final session = await repo.loginWithApple();

    expect(session, isNotNull);
    expect(apple.calls, 1);
    expect(session!.jwt, 'access-1'); // server minted via _pair()
    expect(tokens.accessSync, 'access-1');
    expect(await tokens.readRefresh(), 'refresh-1');
    // The server's expiry is persisted so the proactive-refresh timer (AUTH-R5) works.
    expect(tokens.expiresAtSync, DateTime.utc(2026, 6, 14, 10, 30));
    repo.dispose();
  });

  test('loginWithApple returns null and writes nothing when cancelled',
      () async {
    final tokens = InMemoryTokenStore();
    final repo = AuthRepository(
        config: _cfg(server.base),
        tokens: tokens,
        apple: FakeAppleSignIn(idToken: null));

    final session = await repo.loginWithApple();

    expect(session, isNull);
    expect(tokens.accessSync, isEmpty);
    expect(await tokens.readRefresh(), isNull);
    repo.dispose();
  });

  test('HttpApiClient retries ONCE after a refresh-on-401, then succeeds',
      () async {
    final tokens = InMemoryTokenStore(access: 'old', refresh: 'refresh-old');
    final repo = AuthRepository(config: _cfg(server.base), tokens: tokens);
    var refreshCalls = 0;
    final client = HttpApiClient(
      _cfg(server.base),
      tokens: tokens,
      onUnauthorized: () async {
        refreshCalls++;
        return repo.refresh(); // mints access-N and persists it
      },
    );
    server.authedCallsToReject = 1; // first /v1/captures → 401, then 200

    final feed = await client.captures(); // 401 → refresh → retry → 200
    expect(feed, isEmpty);
    expect(refreshCalls, 1); // refreshed exactly once
    expect(server.refreshCount, 1);
    await client.dispose();
    repo.dispose();
  });
}
