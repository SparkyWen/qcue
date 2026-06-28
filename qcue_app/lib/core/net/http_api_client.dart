// QCue S4 — the REAL network client behind the UNCHANGED [QcueApiClient] seam
// (replaces StubApiClient at bootstrap). It speaks the app-server's REST/RPC
// surface over `package:http` and its streaming surfaces over the hand-rolled
// [SseClient] (recall/wiki-query/dream/ingest). The bidirectional WSS turn
// channel ([WsApiClient]) is built alongside but recall/dream stream over SSE
// this milestone (per the plan).
//
// Canonical wire rules enforced here:
//   - a bearer JWT (`Authorization: Bearer <access>`) on every REST/RPC call;
//   - request bodies are `deny_unknown_fields`-safe (only the fields the Rust
//     DTOs accept — see app_server_protocol::v1);
//   - non-2xx + JSON-RPC-lite `{error:{code,message}}` map to typed [RpcError];
//     `-32001` (overload / cost-cap) is RETRIED with a capped backoff+jitter;
//   - the SSE streams authenticate with the JWT as `?token=` (pitfall #15) and
//     replay-on-reconnect via the last seq;
//   - the client sends ONLY the JWT — never a `tenant_id` (the server's RLS owns
//     isolation), and the vault read carries only the masked `key_hint`.
import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:http/http.dart' as http;

import '../models/app_release_manifest.dart';
import '../models/protocol_models.dart';
import '../models/recall_conversation.dart';
import '../models/runtime_event.dart';
import '../models/sse_event.dart';
import '../session/session_provider.dart' show Session;
import 'capture_query.dart';
import '../sync/sync_dtos.dart';
import 'qcue_api_client.dart';
import 'qcue_config.dart';
import 'sse_client.dart';
import 'ws_client.dart';

/// The real [QcueApiClient]. Construct with a [QcueConfig] + a [TokenStore]; for
/// tests an [SseTransport] / [http.Client] can be injected.
class HttpApiClient implements QcueApiClient {
  HttpApiClient(
    this._config, {
    required TokenStore tokens,
    http.Client? httpClient,
    SseTransport? sseTransport,
    Random? random,
    Future<bool> Function()? onUnauthorized,
        // ignore: prefer_initializing_formals
      })  : _tokens = tokens,
        // ignore: prefer_initializing_formals
        _onUnauthorized = onUnauthorized,
        _http = httpClient ?? http.Client(),
        _rng = random ?? Random() {
    _sse = SseClient(
      sseTransport ?? HttpSseTransport(client: _http),
      token: () => _tokens.accessSync,
      // AUTH-R4: a streaming setup-401 routes through the SAME refresh-on-401
      // hook the REST path uses (the single-flight AuthRepository.refresh()),
      // then reconnects once with the rotated bearer.
      onUnauthorized: onUnauthorized,
    );
  }

  final QcueConfig _config;
  final TokenStore _tokens;
  final http.Client _http;
  final Random _rng;
  late final SseClient _sse;

  /// Task 5: refresh-on-401. On a `-32002`/401 the client invokes this once; if
  /// it returns true (a refresh minted a new bearer) the request is retried
  /// ONCE. If it returns false (or is unset) the 401 propagates so the caller /
  /// router redirects to /login. Wired at bootstrap to [AuthRepository.refresh].
  final Future<bool> Function()? _onUnauthorized;

  final _connState = StreamController<ApiConnectionState>.broadcast();

  // ── retry tuning for the -32001 backpressure signal (S4-R22) ──
  static const _maxRetries = 4;
  static const _baseDelay = Duration(milliseconds: 120);
  static const _maxDelay = Duration(seconds: 5);

  @override
  Stream<ApiConnectionState> get connectionState => _connState.stream;

  @override
  Stream<RuntimeEventEnvelope> events({required String threadId}) =>
      const Stream<RuntimeEventEnvelope>.empty();

  // ── auth ──

  /// `POST /v1/auth/login` → stores the access+refresh pair, returns a [Session].
  /// A keyless session is valid (the BYOK key is added separately, master §4).
  Future<Session> login(String email, String password) async {
    final body = await _postJson('/v1/auth/login', {
      'email': email,
      'password': password,
    });
    final access = body['access_jwt'] as String;
    final refresh = body['refresh_jwt'] as String? ?? '';
    await _tokens.write(access: access, refresh: refresh);
    return Session(jwt: access, email: email, hasKey: false);
  }

  @override
  Future<Map<String, dynamic>> request(
    String method, {
    Map<String, dynamic> params = const {},
  }) async {
    // A generic RPC escape hatch over REST (POST /v1/rpc/<method>). Kept thin —
    // the typed methods below are the real surface.
    assert(!params.containsKey('tenant_id'), 'never send tenant_id (RLS owns it)');
    return _postJson('/v1/rpc/$method', params);
  }

  // ── Capture ──

  @override
  Future<Idea> capture({
    required String body,
    required String origin,
    String? idempotencyKey,
    double? lat,
    double? lng,
    double? accuracyM,
    DateTime? capturedAt,
  }) async {
    // deny_unknown_fields-safe: exactly the CaptureParams fields the server reads.
    // The idempotency key rides as a HEADER (not a body field) so the server
    // dedups `(tenant, Idempotency-Key)` without the deny_unknown_fields DTO
    // having to carry it — a retried flush of the same queued row is a no-op.
    // LOC-R1: location fields are omitted when null so a location-free capture
    // is byte-identical to before.
    // Part F / LOC-R3: send the PRECISE action-time `captured_at` (UTC ISO-8601)
    // when supplied so the server COALESCEs it over its receive time — an offline
    // capture flushed later keeps the instant it was made, not the flush time.
    // Omitted when null so a captured_at-free capture is byte-identical to before.
    final res = await _postJson(
      '/v1/capture',
      {
        'kind': 'text',
        'body': body,
        'origin': origin,
        if (lat != null) 'lat': lat,
        if (lng != null) 'lng': lng,
        if (accuracyM != null) 'loc_accuracy_m': accuracyM,
        if (capturedAt != null) 'captured_at': capturedAt.toUtc().toIso8601String(),
      },
      extraHeaders: (idempotencyKey != null && idempotencyKey.isNotEmpty)
          ? {'Idempotency-Key': idempotencyKey}
          : null,
    );
    // The server returns {idea_id, ingest_job_id}; the freshly-created idea is
    // `pending` (ingestion advances it asynchronously server-side).
    return Idea(
      id: res['idea_id'] as String,
      tenantId: '',
      userId: '',
      kind: IdeaKind.text,
      body: body,
      origin: origin,
      ingestState: IngestState.pending,
      capturedAt: capturedAt ?? DateTime.now().toUtc(),
      lat: lat,
      lng: lng,
      locAccuracyM: accuracyM,
    );
  }

  @override
  Future<String> transcribe({required List<int> audio, String? language}) async {
    // Cloud STT (D4): the server returns {transcript, provider, success, error} with HTTP 200 even on a
    // provider failure (envelope-never-raise, S1-R82). Surface that error instead of swallowing it.
    final res = await _postJson('/v1/transcribe', {
      'audio_b64': base64Encode(audio),
      if (language != null) 'language': language,
    });
    final success = res['success'] as bool? ?? false;
    if (!success) {
      final error = (res['error'] as String?)?.trim() ?? '';
      final kind = _looksLikeMissingKey(error)
          ? TranscribeErrorKind.noKey
          : TranscribeErrorKind.provider;
      throw TranscribeException(
          error.isEmpty ? 'transcription failed' : error,
          kind: kind);
    }
    return res['transcript'] as String? ?? '';
  }

  /// The server's no-key envelope reads "no OpenAI key configured — add one in Settings…".
  bool _looksLikeMissingKey(String error) {
    final e = error.toLowerCase();
    return e.contains('no openai key') || e.contains('key configured');
  }

  @override
  Future<List<Idea>> captures({DateTime? day}) async {
    var path = '/v1/captures';
    if (day != null) {
      final (start, end) = utcDayBounds(day);
      path = '/v1/captures?start=${Uri.encodeQueryComponent(start.toIso8601String())}'
          '&end=${Uri.encodeQueryComponent(end.toIso8601String())}';
    }
    final res = await _getJson(path);
    final rows = (res['captures'] as List? ?? const []).cast<Map>();
    return rows.map((r) => Idea.fromJson(_fillIdea(r.cast<String, dynamic>()))).toList();
  }

  @override
  Future<Idea?> captureDetail(String id) async {
    final res = await _getJsonNullable('/v1/captures/$id');
    return res == null ? null : Idea.fromJson(_fillIdea(res));
  }

  @override
  Future<void> updateCapture(String id, {String? body, double? lat, double? lng, double? locAccuracyM}) async {
    await _patch('/v1/captures/$id', {
      if (body != null) 'body': body,
      if (lat != null) 'lat': lat,
      if (lng != null) 'lng': lng,
      if (locAccuracyM != null) 'loc_accuracy_m': locAccuracyM,
    });
  }

  @override
  Future<void> deleteCapture(String id) async => _delete('/v1/captures/$id');

  // ── Wiki ──

  @override
  Future<List<WikiPage>> wikiIndex() async {
    final res = await _getJson('/v1/wiki/pages');
    final rows = (res['pages'] as List? ?? const []).cast<Map>();
    return rows.map((r) => WikiPage.fromJson(r.cast<String, dynamic>())).toList();
  }

  @override
  Future<WikiPage?> wikiPage(String slug) async {
    final res = await _getJsonNullable('/v1/wiki/pages/$slug');
    if (res == null) return null;
    return WikiPage.fromJson(res);
  }

  // ── App update (release manifest) ──

  @override
  Future<AppReleaseManifest> fetchReleaseManifest(String platform) async {
    // AU-R6 — unauthenticated public metadata; the bearer header is harmless (the server ignores it).
    final r = await _send(
        () => _http.get(_config.uri('/v1/app/release', {'platform': platform}), headers: _authHeaders()));
    final json = _decode(r);
    return json == null ? AppReleaseManifest.none : AppReleaseManifest.fromJson(json);
  }

  // ── Recall (SSE) ──

  @override
  Stream<SseEvent> recallStream(
    String question, {
    String? threadId,
    String? provider,
    String? model,
    String? effort,
  }) {
    // REC-R7: continue reuses the conversation id; a new chat mints a fresh thread.
    final thread = threadId ?? _newUuidV7();
    // v0.2.2: a per-recall model/effort override rides as query params; omitted
    // when null so a default recall is byte-identical to before.
    final params = <String, String>{
      'q': question,
      if (provider != null) 'provider': provider,
      if (model != null) 'model': model,
      if (effort != null) 'effort': effort,
    };
    final url = _config.uri('/v1/recall/$thread/stream', params).toString();
    return _sse.stream(url);
  }

  @override
  Future<List<ConversationSummary>> listConversations() async {
    final res = await _getJson('/v1/conversations');
    final rows = (res['conversations'] as List? ?? const []).cast<Map>();
    return rows
        .map((r) => ConversationSummary.fromJson(r.cast<String, dynamic>()))
        .toList();
  }

  @override
  Future<List<ConversationMessage>> getConversationMessages(String threadId) async {
    final res = await _getJson('/v1/conversations/$threadId/messages');
    final rows = (res['messages'] as List? ?? const []).cast<Map>();
    return rows
        .map((r) => ConversationMessage.fromJson(r.cast<String, dynamic>()))
        .toList();
  }

  // ── Activity ──

  @override
  Future<List<Approval>> approvals() async {
    final res = await _getJson('/v1/approvals');
    final rows = (res['approvals'] as List? ?? const []).cast<Map>();
    return rows.map((r) => Approval.fromJson(r.cast<String, dynamic>())).toList();
  }

  @override
  Future<void> respondApproval(String id, bool approve) async {
    await _postJson('/v1/approvals/$id', {'approve': approve});
  }

  @override
  Future<int> runIngest() async {
    final res = await _postJson('/v1/ingest/run', const {});
    return (res['enqueued'] as num?)?.toInt() ?? 0;
  }

  @override
  Future<List<JobRow>> jobs() async {
    final res = await _getJson('/v1/jobs');
    final rows = (res['jobs'] as List? ?? const []).cast<Map>();
    return rows.map((r) => JobRow.fromJson(r.cast<String, dynamic>())).toList();
  }

  @override
  Future<int> todayCostMicros() async {
    final res = await _getJson('/v1/cost/today');
    return (res['cost_micros'] as num?)?.toInt() ?? 0;
  }

  @override
  Stream<SseEvent> dreamEvents(String jobId) {
    final url = _config.uri('/v1/dream/$jobId/stream').toString();
    return _sse.stream(url);
  }

  @override
  Future<void> cancelJob(String jobId) async {
    await _postJson('/v1/jobs/$jobId/cancel', const {});
  }

  // ── Settings ──

  @override
  Future<List<ProviderCredential>> credentials() async {
    final res = await _getJson('/v1/settings/keys');
    final rows = (res['keys'] as List? ?? const []).cast<Map>();
    // The vault read carries ONLY the masked key_hint (the secret never crosses).
    return rows
        .map((r) => ProviderCredential.fromJson(r.cast<String, dynamic>()))
        .toList();
  }

  @override
  Future<ProviderCredential> putKey(String provider, String key) async {
    // The plaintext crosses on WRITE only; the response carries the hint. The
    // body matches the server's PutKey DTO (deny_unknown_fields).
    final res = await _putJson('/v1/settings/keys', {
      'provider': provider,
      'key': key,
    });
    return ProviderCredential.fromJson(res);
  }

  @override
  Future<void> deleteKey(String provider) async {
    // The server deletes by credential id (`/v1/settings/keys/{id}`), so resolve
    // provider → id from the masked list first.
    final res = await _getJson('/v1/settings/keys');
    final rows = (res['keys'] as List? ?? const []).cast<Map>();
    final row = rows.cast<Map<String, dynamic>?>().firstWhere(
          (r) => r != null && r['provider'] == provider,
          orElse: () => null,
        );
    final id = row?['id'] as String?;
    if (id == null) return; // already absent — idempotent delete
    await _delete('/v1/settings/keys/$id');
  }

  @override
  Future<void> deleteAccount() async {
    // DELETE /v1/account — the server revokes sessions and purges the tenant
    // (cascading to all synced data + keys). _delete attaches the bearer token.
    await _delete('/v1/account');
  }

  @override
  Future<List<String>> fetchModels(String provider) async {
    final res = await _getJson('/v1/settings/models/$provider');
    return (res['models'] as List? ?? const []).cast<String>();
  }

  @override
  Future<String?> activeModel(String provider) async {
    final res = await _getJsonNullable('/v1/settings/models/$provider/active');
    return res?['model'] as String?;
  }

  @override
  Future<void> setActiveModel(String provider, String model) async {
    await _putJson('/v1/settings/models/$provider/active', {'model': model});
  }

  @override
  Future<List<CostLedgerRow>> costLedger() async {
    final res = await _getJson('/v1/cost/ledger');
    final rows = (res['rows'] as List? ?? const []).cast<Map>();
    return rows.map((r) => CostLedgerRow.fromJson(r.cast<String, dynamic>())).toList();
  }

  @override
  Future<bool> serverDream() async {
    final res = await _getJson('/v1/settings/dream');
    return res['enabled'] as bool? ?? false;
  }

  @override
  Future<void> setServerDream(bool on) async {
    await _putJson('/v1/settings/dream', {'enabled': on});
  }

  @override
  Future<SttProviders> sttProviders() async {
    final res = await _getJson('/v1/transcribe/providers');
    return SttProviders(
      selected: res['selected'] as String?,
      available: (res['available'] as List? ?? const []).cast<String>(),
      allCapable: (res['all_capable'] as List? ?? const []).cast<String>(),
    );
  }

  @override
  Future<void> setSttProvider(String? provider) async {
    await _putJson('/v1/settings/stt-provider', {'provider': provider ?? 'auto'});
  }

  // ── Sync (Phase 1: read sync) ──

  @override
  Future<DeviceReg> registerDevice(String platform) async {
    // deny_unknown_fields-safe: exactly the RegisterParams fields the server
    // reads. The server returns {device_id, site_id}. Never sends tenant_id.
    final res = await _postJson('/v1/sync/register', {
      'platform': platform,
      'display_name': platform,
    });
    return DeviceReg.fromJson(res);
  }

  @override
  Future<SyncDelta> pullSync({required int since}) async {
    // GET /v1/sync/pull?since=<seq>: a snapshot bootstrap (since:0) or
    // incremental ops by seq. The client never sends tenant_id (RLS owns it).
    final res = await _getJson('/v1/sync/pull?since=$since');
    return SyncDelta.fromJson(res);
  }

  @override
  Future<void> dispose() async {
    await _connState.close();
    _http.close();
  }

  // ── HTTP plumbing ────────────────────────────────────────────────────────

  Map<String, String> _authHeaders({bool json = false}) => {
        'Authorization': 'Bearer ${_tokens.accessSync}',
        if (json) 'Content-Type': 'application/json',
        'Accept': 'application/json',
      };

  Future<Map<String, dynamic>> _getJson(String path) async {
    final r = await _send(() =>
        _http.get(_config.uri(path), headers: _authHeaders()));
    return _decode(r)!;
  }

  /// A GET that tolerates a 404 (returns null) — for `wikiPage`/`activeModel`.
  Future<Map<String, dynamic>?> _getJsonNullable(String path) async {
    final r = await _send(
      () => _http.get(_config.uri(path), headers: _authHeaders()),
      allowNotFound: true,
    );
    if (r.statusCode == 404) return null;
    return _decode(r);
  }

  Future<Map<String, dynamic>> _postJson(
    String path,
    Map<String, dynamic> body, {
    Map<String, String>? extraHeaders,
  }) async {
    final r = await _send(() => _http.post(
          _config.uri(path),
          headers: {
            ..._authHeaders(json: true),
            if (extraHeaders != null) ...extraHeaders,
          },
          body: jsonEncode(body),
        ));
    return _decode(r) ?? const {};
  }

  Future<Map<String, dynamic>> _putJson(
      String path, Map<String, dynamic> body) async {
    final r = await _send(() => _http.put(
          _config.uri(path),
          headers: _authHeaders(json: true),
          body: jsonEncode(body),
        ));
    return _decode(r) ?? const {};
  }

  Future<Map<String, dynamic>> _patch(String path, Map<String, dynamic> body) async {
    final r = await _send(() => _http.patch(
          _config.uri(path),
          headers: _authHeaders(json: true),
          body: jsonEncode(body),
        ));
    return _decode(r) ?? const {};
  }

  Future<void> _delete(String path) async {
    await _send(() => _http.delete(_config.uri(path), headers: _authHeaders()));
  }

  /// Run [op], retrying a `-32001` backpressure response with a capped
  /// exponential backoff + full jitter (S4-R22). Non-backpressure errors throw
  /// immediately; a 2xx (or a tolerated 404) returns the response.
  Future<http.Response> _send(
    Future<http.Response> Function() op, {
    bool allowNotFound = false,
  }) async {
    var triedRefresh = false;
    for (var attempt = 0;; attempt++) {
      final r = await op();
      if (r.statusCode >= 200 && r.statusCode < 300) return r;
      if (allowNotFound && r.statusCode == 404) return r;
      final err = _errorOf(r);
      if (err.isBackpressure && attempt < _maxRetries) {
        await Future<void>.delayed(_backoff(attempt));
        continue;
      }
      // Task 5: a 401 (the JWT was rejected/expired) → try ONE refresh, then
      // retry the request with the freshly-minted bearer. If the refresh fails,
      // the 401 propagates so the caller / router redirects to /login.
      if (err.isUnauthorized && !triedRefresh && _onUnauthorized != null) {
        triedRefresh = true;
        final refreshed = await _onUnauthorized();
        if (refreshed) continue; // retry once with the new token
      }
      throw err;
    }
  }

  /// Map a non-2xx response to a typed [RpcError], preferring the server's
  /// JSON-RPC-lite `{error:{code,message}}` body, else deriving the code from
  /// the HTTP status (503 → overload/-32001, 401 → -32002).
  RpcError _errorOf(http.Response r) {
    try {
      // utf8 (not latin-1 r.body) so a localized server error message isn't surfaced as mojibake.
      final decoded = jsonDecode(utf8.decode(r.bodyBytes));
      if (decoded is Map && decoded['error'] is Map) {
        final e = (decoded['error'] as Map).cast<String, dynamic>();
        return RpcError(
          (e['code'] as num?)?.toInt() ?? _codeForStatus(r.statusCode),
          e['message'] as String? ?? '',
        );
      }
    } catch (_) {/* fall through to status-derived code */}
    return RpcError(_codeForStatus(r.statusCode), 'HTTP ${r.statusCode}');
  }

  static int _codeForStatus(int status) => switch (status) {
        503 => -32001, // overload / cost-cap (backpressure)
        401 => -32002, // unauthorized
        400 || 422 => -32602, // invalid params
        _ => -32603, // internal
      };

  Duration _backoff(int attempt) {
    final exp = _baseDelay.inMilliseconds * (1 << attempt);
    final capped = min(exp, _maxDelay.inMilliseconds);
    // full jitter: uniform in [0, capped]
    return Duration(milliseconds: _rng.nextInt(capped + 1));
  }

  Map<String, dynamic>? _decode(http.Response r) {
    // The server sends `application/json` WITHOUT a charset, so `r.body` would decode as latin-1 and
    // mangle UTF-8 (Chinese → 乱码). Decode the raw bytes as UTF-8 explicitly (application/json IS utf-8).
    if (r.bodyBytes.isEmpty) return const {};
    final decoded = jsonDecode(utf8.decode(r.bodyBytes));
    return decoded is Map ? decoded.cast<String, dynamic>() : null;
  }

  /// The `/v1/captures` feed omits tenant/user (RLS-scoped) — backfill the
  /// non-null model fields the [Idea] decoder requires.
  Map<String, dynamic> _fillIdea(Map<String, dynamic> r) => {
        'tenant_id': '',
        'user_id': '',
        'origin': 'capture',
        ...r,
      };

  /// A v7-flavoured UUID (time-ordered prefix) for a fresh recall thread id.
  String _newUuidV7() {
    final ms = DateTime.now().toUtc().millisecondsSinceEpoch;
    String hex(int v, int width) => v.toRadixString(16).padLeft(width, '0');
    final timeHi = hex((ms >> 16) & 0xffffffff, 8);
    final timeLo = hex(ms & 0xffff, 4);
    final r1 = hex(0x7000 | _rng.nextInt(0x1000), 4); // version 7
    final r2 = hex(0x8000 | _rng.nextInt(0x4000), 4); // variant
    final r3 = hex(_rng.nextInt(0x10000), 4) + hex(_rng.nextInt(0x100000000), 8);
    return '$timeHi-$timeLo-$r1-$r2-$r3';
  }
}
