// QCue S4 — the real network client behind the UNCHANGED [QcueApiClient] seam,
// exercised end-to-end against an in-process `dart:io` HttpServer fake (no real
// app-server, no network). Asserts the canonical wire contracts:
//   - bearer JWT attached to every REST/RPC call (S4-R19);
//   - capture / keys / approvals round-trip against the real route shapes;
//   - the vault read returns ONLY `key_hint` (the secret never crosses, S4-R46);
//   - a `-32001` overload retries with backoff+jitter, then succeeds (S4-R22);
//   - the recall SSE stream decodes the §3.4 taxonomy IN ORDER, skips an
//     injected unknown `event`, and replays from the last seq on a simulated
//     reconnect (S4-R20/R21/R23/R24).
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/net/http_api_client.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/net/qcue_config.dart';

/// A scriptable in-process app-server stand-in. Records the Authorization header
/// of every request so the bearer-auth assertion can inspect it.
class FakeServer {
  late final HttpServer _server;
  final authHeaders = <String, String?>{}; // path → Authorization
  String? captureIdempotencyKey; // last /v1/capture Idempotency-Key header
  int captureCount = 0;
  int overloadCallsRemaining = 0; // how many times /v1/jobs/x returns -32001
  final approvals = <Map<String, dynamic>>[
    {
      'id': 'ap-1',
      'action': 'wiki_merge',
      'status': 'pending',
      'requested_by': 'dream',
      'subject_ref': {'target_slug': 'recall-architecture'},
    },
  ];
  // Per-thread SSE state: how many times the recall stream has been opened (to
  // simulate a drop-then-replay) keyed by thread.
  final recallOpens = <String, int>{};

  Future<void> start() async {
    _server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    _server.listen(_handle);
  }

  String get base => 'http://127.0.0.1:${_server.port}';
  Future<void> stop() => _server.close(force: true);

  Future<void> _handle(HttpRequest req) async {
    final path = req.uri.path;
    authHeaders[path] = req.headers.value(HttpHeaders.authorizationHeader);
    final res = req.response;

    Future<void> json(Object body, {int status = 200}) async {
      res.statusCode = status;
      res.headers.contentType = ContentType.json;
      res.write(jsonEncode(body));
      await res.close();
    }

    // ── auth ──
    if (path == '/v1/auth/login' && req.method == 'POST') {
      await utf8.decoder.bind(req).join();
      return json({
        'access_jwt': 'access-jwt-123',
        'refresh_jwt': 'refresh-jwt-456',
        'expires_at': '2026-06-13T10:30:00Z',
      });
    }

    // ── capture ──
    if (path == '/v1/capture' && req.method == 'POST') {
      captureIdempotencyKey = req.headers.value('Idempotency-Key');
      final body = jsonDecode(await utf8.decoder.bind(req).join())
          as Map<String, dynamic>;
      captureCount++;
      // the server denies unknown fields → assert the request is minimal.
      expect(body.keys.toSet(), {'kind', 'body', 'origin'});
      return json({
        'idea_id': '00000000-0000-7000-8000-00000000c0de',
        'ingest_job_id': '00000000-0000-7000-8000-00000000b00b',
      });
    }
    if (path == '/v1/captures' && req.method == 'GET') {
      return json({
        'captures': [
          {
            'id': 'i-1',
            'kind': 'text',
            'body': 'a captured thought',
            'ingest_state': 'ingested',
            'captured_at': '2026-06-13T09:00:00Z',
          },
        ],
      });
    }

    // ── vault keys ──
    if (path == '/v1/settings/keys' && req.method == 'PUT') {
      final body = jsonDecode(await utf8.decoder.bind(req).join())
          as Map<String, dynamic>;
      // the plaintext key crosses ON WRITE only; the response carries the hint.
      expect(body['key'], 'sk-secret-plaintext-7890');
      return json({
        'id': 'cred-1',
        'provider': body['provider'],
        'label': body['label'],
        'key_hint': 'sk-…7890',
        'status': 'ok',
      });
    }
    if (path == '/v1/settings/keys' && req.method == 'GET') {
      return json({
        'keys': [
          {
            'id': 'cred-1',
            'provider': 'openai',
            'label': null,
            'key_hint': 'sk-…7890',
            'status': 'ok',
            'cooldown_until': null,
          },
        ],
      });
    }
    if (path == '/v1/settings/keys/cred-1' && req.method == 'DELETE') {
      return json({'deleted': 'cred-1'});
    }

    // ── approvals (ingest review — D13) ──
    if (path == '/v1/approvals' && req.method == 'GET') {
      return json({'approvals': approvals});
    }
    if (path == '/v1/approvals/ap-1' && req.method == 'POST') {
      final body = jsonDecode(await utf8.decoder.bind(req).join())
          as Map<String, dynamic>;
      expect(body['approve'], isA<bool>());
      approvals.removeWhere((a) => a['id'] == 'ap-1');
      return json({'ok': true});
    }

    // ── jobs list with a -32001 backpressure prefix (S4-R22) ──
    if (path == '/v1/jobs' && req.method == 'GET') {
      if (overloadCallsRemaining > 0) {
        overloadCallsRemaining--;
        return json(
          {'error': {'code': -32001, 'message': 'overloaded'}},
          status: 503,
        );
      }
      return json({
        'jobs': [
          {'id': 'j-1', 'kind': 'dream', 'state': 'done'},
        ],
      });
    }

    // ── recall SSE stream ──
    if (path.startsWith('/v1/recall/') && path.endsWith('/stream')) {
      final thread = path.split('/')[3];
      final opens = (recallOpens[thread] ?? 0) + 1;
      recallOpens[thread] = opens;
      final sinceSeq =
          int.tryParse(req.uri.queryParameters['since_seq'] ?? '0') ?? 0;
      // token MUST be present on the query (EventSource can't send headers).
      expect(req.uri.queryParameters['token'], isNotEmpty);
      res.statusCode = 200;
      res.headers.set(HttpHeaders.contentTypeHeader, 'text/event-stream');
      res.headers.set('Cache-Control', 'no-cache');

      void frame(int seq, String event, Map<String, dynamic> payload) {
        if (seq <= sinceSeq) return; // replay ring honours since_seq
        final env = {
          'schema_version': 1,
          'thread_id': thread,
          'turn_id': null,
          'seq': seq,
          'event': event,
          'payload': payload,
        };
        res.write('id: $seq\ndata: ${jsonEncode(env)}\n\n');
      }

      if (opens == 1) {
        // first connection: emit through seq 3, inject an unknown at seq 2,
        // then abruptly close mid-answer (no `done`) to force a reconnect.
        frame(1, 'session_started', {'mode': 'recall'});
        frame(2, 'from_the_future', {'whatever': true}); // unknown → skipped
        frame(3, 'message_delta', {'delta': 'You decided '});
        await res.flush();
        await res.close(); // drop mid-stream
      } else {
        // reconnect: the ring re-sends seq 3 (already seen → deduped) + the rest.
        frame(3, 'message_delta', {'delta': 'You decided '});
        frame(4, 'message_delta', {'delta': 'against embeddings.'});
        frame(
          5,
          'citation',
          {'rel_path': 'recall-architecture.md', 'start_line': 42, 'end_line': 47},
        );
        frame(6, 'usage', {'input': 1280, 'output': 64, 'reasoning': 22});
        frame(7, 'done', {'ok': true});
        await res.flush();
        await res.close();
      }
      return;
    }

    // ── transcribe (D4): echoes a scripted envelope so the client's success/error
    //    handling can be asserted. `language:"fail"` ⇒ a provider failure envelope.
    if (path == '/v1/transcribe' && req.method == 'POST') {
      final body = jsonDecode(await utf8.decoder.bind(req).join())
          as Map<String, dynamic>;
      if (body['language'] == 'fail') {
        return json({
          'transcript': '',
          'provider': 'openai',
          'success': false,
          'error': 'openai stt 400: model not found',
        });
      }
      return json({
        'transcript': 'hello world',
        'provider': 'openai',
        'success': true,
        'error': null,
      });
    }

    res.statusCode = 404;
    await res.close();
  }
}

QcueConfig _cfg(String base) => QcueConfig(baseUrl: base);

void main() {
  late FakeServer server;
  late HttpApiClient client;

  setUp(() async {
    server = FakeServer();
    await server.start();
    client = HttpApiClient(
      _cfg(server.base),
      tokens: InMemoryTokenStore(access: 'test-access-jwt'),
    );
  });

  tearDown(() async {
    await client.dispose();
    await server.stop();
  });

  test('S4-R52: login stores the access JWT and uses it as the bearer', () async {
    final fresh = HttpApiClient(_cfg(server.base), tokens: InMemoryTokenStore());
    final session = await fresh.login('a@b.co', 'pw');
    expect(session.jwt, 'access-jwt-123');
    // a subsequent authed call carries the freshly-stored bearer.
    await fresh.captures();
    expect(server.authHeaders['/v1/captures'], 'Bearer access-jwt-123');
    await fresh.dispose();
  });

  test('S4-R19: every REST call attaches the bearer JWT', () async {
    await client.captures();
    expect(server.authHeaders['/v1/captures'], 'Bearer test-access-jwt');
  });

  test('capture round-trips through the real /v1/capture shape', () async {
    final idea = await client.capture(body: 'hello', origin: 'capture');
    expect(server.captureCount, 1);
    expect(idea.body, 'hello');
    expect(idea.ingestState, IngestState.pending);
    expect(server.authHeaders['/v1/capture'], 'Bearer test-access-jwt');
  });

  test('Task 6: capture sends the Idempotency-Key header when supplied',
      () async {
    await client.capture(
        body: 'hello', origin: 'capture', idempotencyKey: 'idem-abc-123');
    expect(server.captureIdempotencyKey, 'idem-abc-123');
  });

  test('Task 6: capture omits the Idempotency-Key header when none is given',
      () async {
    await client.capture(body: 'hello', origin: 'capture');
    expect(server.captureIdempotencyKey, isNull);
  });

  test('captures decodes the reverse-chron feed', () async {
    final feed = await client.captures();
    expect(feed.single.body, 'a captured thought');
    expect(feed.single.ingestState, IngestState.ingested);
  });

  test('S4-R46: putKey sends the plaintext but the vault read returns only the '
      'key_hint (the secret never crosses back)', () async {
    final cred = await client.putKey('openai', 'sk-secret-plaintext-7890');
    expect(cred.provider, 'openai');
    expect(cred.keyHint, 'sk-…7890');

    final creds = await client.credentials();
    expect(creds.single.provider, 'openai');
    expect(creds.single.keyHint, 'sk-…7890');
    // the model has no field that could carry a plaintext secret.
    expect(creds.single.toJson().containsKey('key'), isFalse);
  });

  test('deleteKey resolves provider → id, then DELETEs by id', () async {
    await client.deleteKey('openai');
    expect(server.authHeaders['/v1/settings/keys/cred-1'], 'Bearer test-access-jwt');
  });

  test('approvals round-trip: list the pending D13 candidates, then respond',
      () async {
    final pending = await client.approvals();
    expect(pending.single.id, 'ap-1');
    expect(pending.single.action, 'wiki_merge');
    await client.respondApproval('ap-1', true);
    expect(await client.approvals(), isEmpty);
  });

  test('D4: transcribe returns the transcript on a success envelope', () async {
    final text = await client.transcribe(audio: const [1, 2, 3]);
    expect(text, 'hello world');
  });

  test('D4: transcribe throws TranscribeException carrying the server error on '
      'a success:false envelope', () async {
    // the fake maps language:"fail" to a {success:false, error} envelope (HTTP 200).
    await expectLater(
      () => client.transcribe(audio: const [1, 2, 3], language: 'fail'),
      throwsA(isA<TranscribeException>()
          .having((e) => e.message, 'message', contains('model not found'))),
    );
  });

  test('S4-R22: a -32001 overload retries with backoff+jitter, then succeeds',
      () async {
    server.overloadCallsRemaining = 2; // two 503s, then a 200
    final jobs = await client.jobs(); // jobs() GETs /v1/jobs (retries on -32001)
    expect(jobs.single.state, JobState.done);
  });

  test(
      'S4-R20/R21/R23/R24: recall SSE decodes the taxonomy in order, skips the '
      'unknown event, and replays from the last seq across a reconnect',
      () async {
    final events = <SseEvent>[];
    final done = Completer<void>();
    late StreamSubscription<SseEvent> sub;
    sub = client.recallStream('embeddings?').listen(
      (e) {
        events.add(e);
        if (e is DoneEvent) {
          done.complete();
          sub.cancel();
        }
      },
    );
    await done.future.timeout(const Duration(seconds: 5));

    // The unknown `from_the_future` event was skipped (forward-compat).
    expect(events.whereType<UnknownEvent>(), isEmpty);
    // The two message deltas arrived exactly once each (no duplicate of seq 3).
    final deltas = events.whereType<MessageDelta>().map((e) => e.text).toList();
    expect(deltas, ['You decided ', 'against embeddings.']);
    // The taxonomy decoded end-to-end.
    expect(events.whereType<SessionStarted>(), isNotEmpty);
    expect(events.whereType<CitationEvent>().single.citation.label,
        'recall-architecture.md:42-47');
    expect(events.whereType<UsageEvent>().single.inputTokens, 1280);
    expect(events.whereType<DoneEvent>(), isNotEmpty);
    // The stream was opened twice (drop → replay-on-reconnect).
    expect(server.recallOpens.values.single, 2);
  });
}
