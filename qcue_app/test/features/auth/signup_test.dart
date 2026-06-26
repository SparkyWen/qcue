// QCue v0.1.1 (WS-A2): the email/password signup repository call. Exercised
// against an in-process app-server stand-in (no real server / no network), the
// SAME pattern as login_test. Matches the deployed Rust contract:
//   - POST /v1/auth/signup {email, password} → 200 {access_jwt, refresh_jwt, …}
//     (the SAME shape as login) ⇒ stores the pair + returns the session;
//   - a duplicate email → 400 "email already registered" ⇒ AuthError.emailTaken;
//   - an unreachable server ⇒ AuthError.network (→ "check Server URL").
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_config.dart';
import 'package:qcue_app/features/auth/auth_repository.dart';

/// A scriptable signup/server stand-in. Mints distinct tokens per call so a
/// rotation is observable; can be told to reject the signup with the server's
/// duplicate-email 400.
class FakeSignupServer {
  late final HttpServer _server;
  bool duplicateEmail = false; // signup → 400 "email already registered"
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

    if (path == '/v1/auth/signup' && req.method == 'POST') {
      final body = jsonDecode(await utf8.decoder.bind(req).join())
          as Map<String, dynamic>;
      // The server uses deny_unknown_fields → ONLY these two keys are sent.
      expect(body.keys.toSet(), {'email', 'password'});
      if (duplicateEmail) {
        return json({'message': 'email already registered'}, status: 400);
      }
      return json(_pair());
    }
    res.statusCode = 404;
    await res.close();
  }
}

QcueConfig _cfg(String base) => QcueConfig(baseUrl: base);

void main() {
  late FakeSignupServer server;

  setUp(() async {
    server = FakeSignupServer();
    await server.start();
  });
  tearDown(() => server.stop());

  test('signup success stores the access+refresh pair and returns the session',
      () async {
    final tokens = InMemoryTokenStore();
    final repo = AuthRepository(config: _cfg(server.base), tokens: tokens);

    final session = await repo.signup('new@b.co', 'pw');

    expect(session.email, 'new@b.co');
    expect(session.jwt, 'access-1');
    expect(tokens.accessSync, 'access-1');
    expect(await tokens.readRefresh(), 'refresh-1');
    repo.dispose();
  });

  test('a duplicate-email 400 surfaces AuthError.emailTaken with the message',
      () async {
    server.duplicateEmail = true;
    final repo = AuthRepository(
        config: _cfg(server.base), tokens: InMemoryTokenStore());
    await expectLater(
      repo.signup('taken@b.co', 'pw'),
      throwsA(isA<AuthError>()
          .having((e) => e.kind, 'kind', AuthErrorKind.emailTaken)
          .having((e) => e.message, 'message',
              contains('email already registered'))),
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
      repo.signup('a@b.co', 'pw'),
      throwsA(isA<AuthError>()
          .having((e) => e.kind, 'kind', AuthErrorKind.network)),
    );
    repo.dispose();
  });
}
