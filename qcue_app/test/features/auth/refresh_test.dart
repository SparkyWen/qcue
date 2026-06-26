// QCue AUTH-R3/R5/R6: AuthRepository.refresh() is single-flight (concurrent
// callers share ONE /v1/auth/refresh), persists expires_at, and clears tokens
// only on a definitive 401 — a network error leaves the pair intact.
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_config.dart';
import 'package:qcue_app/features/auth/auth_repository.dart';

/// A counting in-process refresh endpoint. Records how many times the refresh
/// route was hit and can be told to delay (so concurrent callers overlap) or to
/// fail with a chosen status.
class _RefreshServer {
  late final HttpServer _server;
  int refreshHits = 0;
  Duration delay = Duration.zero;
  int status = 200;
  bool malformed = false;

  Future<void> start() async {
    _server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    _server.listen((req) async {
      if (req.uri.path == '/v1/auth/refresh') {
        refreshHits++;
        await utf8.decoder.bind(req).join();
        if (delay > Duration.zero) await Future<void>.delayed(delay);
        final res = req.response..statusCode = status;
        if (status == 200) {
          if (malformed) {
            // A 200 with a non-JSON body (e.g. a proxy/gateway error page) —
            // jsonDecode throws; the single-flight refresh must still resolve.
            res.write('<html>not json at all {{{');
          } else {
            res.headers.contentType = ContentType.json;
            res.write(jsonEncode({
              'access_jwt': 'new-access-$refreshHits',
              'refresh_jwt': 'new-refresh-$refreshHits',
              'expires_at': '2026-06-16T11:00:00Z',
            }));
          }
        }
        await res.close();
        return;
      }
      req.response.statusCode = 404;
      await req.response.close();
    });
  }

  String get base => 'http://127.0.0.1:${_server.port}';
  Future<void> stop() => _server.close(force: true);
}

AuthRepository _repo(String base, TokenStore tokens) =>
    AuthRepository(config: QcueConfig(baseUrl: base), tokens: tokens);

void main() {
  test('AUTH-R3: concurrent refresh() calls collapse to ONE network call',
      () async {
    final server = _RefreshServer()..delay = const Duration(milliseconds: 50);
    await server.start();
    final tokens = InMemoryTokenStore(access: 'old', refresh: 'old-refresh');
    final repo = _repo(server.base, tokens);

    // Fire 5 refreshes concurrently; the single-flight cache must coalesce them.
    final results = await Future.wait([for (var i = 0; i < 5; i++) repo.refresh()]);
    expect(results, everyElement(isTrue));
    expect(server.refreshHits, 1, reason: 'exactly one /v1/auth/refresh for N concurrent callers');
    expect(tokens.accessSync, 'new-access-1');
    expect(tokens.expiresAtSync, DateTime.utc(2026, 6, 16, 11, 0));
    repo.dispose();
    await server.stop();
  });

  test('AUTH-R6: a network error during refresh does NOT clear the token pair',
      () async {
    // No server: the host is unreachable → AuthError.network, NOT a 401.
    final tokens = InMemoryTokenStore(access: 'a', refresh: 'r');
    final repo = _repo('http://127.0.0.1:1', tokens); // port 1 → connection refused
    final ok = await repo.refresh();
    expect(ok, isFalse);
    expect(tokens.accessSync, 'a', reason: 'network failure must not wipe the access token');
    expect(await tokens.readRefresh(), 'r', reason: 'network failure must not wipe the refresh token');
    repo.dispose();
  });

  test('AUTH-R6: a definitive 401 DOES clear the token pair', () async {
    final server = _RefreshServer()..status = 401;
    await server.start();
    final tokens = InMemoryTokenStore(access: 'a', refresh: 'r');
    final repo = _repo(server.base, tokens);
    final ok = await repo.refresh();
    expect(ok, isFalse);
    expect(tokens.accessSync, isEmpty, reason: 'a dead refresh token is cleared');
    expect(await tokens.readRefresh(), isNull);
    repo.dispose();
    await server.stop();
  });

  test('AUTH-R3/R6: a 200 with a malformed body resolves to false, never throws, keeps tokens',
      () async {
    // A non-401 failure (jsonDecode FormatException) must still resolve the
    // single-flight future to `false` — never let it reject, or concurrent
    // awaiters would throw instead of routing to a retry. Tokens stay intact.
    final server = _RefreshServer()..malformed = true;
    await server.start();
    final tokens = InMemoryTokenStore(access: 'a', refresh: 'r');
    final repo = _repo(server.base, tokens);
    // Two concurrent callers share the in-flight future; neither may throw.
    final results = await Future.wait([repo.refresh(), repo.refresh()]);
    expect(results, everyElement(isFalse));
    expect(tokens.accessSync, 'a', reason: 'a malformed body must not wipe the access token');
    expect(await tokens.readRefresh(), 'r', reason: 'a malformed body must not wipe the refresh token');
    repo.dispose();
    await server.stop();
  });
}
