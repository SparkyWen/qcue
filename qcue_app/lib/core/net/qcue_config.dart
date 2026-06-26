// QCue S4: the network client's configuration + the JWT/token store seam.
//
// [QcueConfig] is the base URL the REST/RPC + SSE + WSS clients build their
// endpoints from (default the deployed cloud app-server, `https://app.qcue.cn`).
// It derives the matching `ws(s)://` origin for the bidirectional turn channel.
//
// [TokenStore] is the access/refresh JWT seam. The default in-memory store is
// used by tests + the boot stub; the platform store (Keychain/Keystore via the
// `secure_storage` channel) is injected at bootstrap so a session survives an
// app restart — the Dart layer holds the JWT but NEVER a provider secret.
import 'dart:async';

/// Immutable network configuration. [baseUrl] has no trailing slash.
class QcueConfig {
  QcueConfig({String baseUrl = defaultBaseUrl})
      : baseUrl = _strip(baseUrl);

  /// The deployed cloud app-server (override via Settings or `--dart-define`).
  static const defaultBaseUrl = 'https://app.qcue.cn';

  final String baseUrl;

  /// True if [url] parses as an absolute http(s) URL with a host — the only
  /// shape we accept as a base URL (rejects garbage, relative, or `ws://`).
  ///
  /// SECURITY (transport): cleartext `http://` is permitted ONLY for loopback /
  /// emulator dev hosts (mirrors android/app/src/main/res/xml/
  /// network_security_config.xml). Any other host MUST be `https://`.
  /// This guard lives in Dart on purpose: the REST `http` client and the `ws://`
  /// turn channel run over `dart:io` sockets, which do NOT honor Android's
  /// network_security_config or iOS ATS — so without this check a Settings-set
  /// or `--dart-define` `http://api.attacker` base would carry the bearer JWT,
  /// the plaintext BYOK key (POST /v1/keys), and `ws://` turn frames in cleartext.
  static bool isValidBaseUrl(String? url) {
    if (url == null) return false;
    final s = url.trim();
    if (s.isEmpty) return false;
    final uri = Uri.tryParse(s);
    if (uri == null) return false;
    if (uri.scheme != 'http' && uri.scheme != 'https') return false;
    if (uri.host.isEmpty) return false;
    if (uri.scheme == 'http' && !_isLoopbackHost(uri.host)) return false;
    return true;
  }

  /// The loopback / emulator dev hosts allowed to use cleartext `http://`.
  /// Kept in lockstep with `network_security_config.xml`'s cleartext allowlist.
  static bool _isLoopbackHost(String host) {
    switch (host.toLowerCase()) {
      case 'localhost':
      case '127.0.0.1':
      case '::1':
      case '10.0.2.2': // Android emulator → host loopback
        return true;
      default:
        return false;
    }
  }

  /// Resolve the effective base URL in priority order (Task 4):
  ///   1. [runtimeOverride] — a Settings-set value (only if it validates);
  ///   2. [buildTimeDefault] — `--dart-define=QCUE_BASE_URL` (only if it
  ///      validates; this is the bootstrap default);
  ///   3. [defaultBaseUrl] — the cloud-server fallback.
  /// An invalid override is ignored (falls through), never used.
  static String resolveBaseUrl({
    String? runtimeOverride,
    String buildTimeDefault = defaultBaseUrl,
  }) {
    if (isValidBaseUrl(runtimeOverride)) return _strip(runtimeOverride!.trim());
    if (isValidBaseUrl(buildTimeDefault)) return _strip(buildTimeDefault.trim());
    return defaultBaseUrl;
  }

  /// The WebSocket origin for the WSS turn channel (`http→ws`, `https→wss`).
  String get wsBaseUrl {
    if (baseUrl.startsWith('https://')) {
      return 'wss://${baseUrl.substring('https://'.length)}';
    }
    if (baseUrl.startsWith('http://')) {
      return 'ws://${baseUrl.substring('http://'.length)}';
    }
    return baseUrl;
  }

  /// Join [path] (a leading-slash absolute path) onto the base URL.
  Uri uri(String path, [Map<String, String>? query]) =>
      Uri.parse('$baseUrl$path').replace(
        queryParameters: (query == null || query.isEmpty) ? null : query,
      );

  static String _strip(String s) =>
      s.endsWith('/') ? s.substring(0, s.length - 1) : s;
}

/// The session-token seam: the access JWT used as the REST bearer + the SSE/WSS
/// `?token=`, and the refresh JWT used to mint a fresh pair.
abstract interface class TokenStore {
  Future<String?> readAccess();
  Future<String?> readRefresh();
  Future<void> write({required String access, required String refresh});
  Future<void> clear();

  /// The currently-cached access token, read synchronously for the SSE/WSS
  /// `?token=` callback (which must produce a value without awaiting). Returns
  /// an empty string before the first [write]/login.
  String get accessSync;

  /// Persist the access token's expiry so the client can schedule a proactive
  /// refresh at ~80% of TTL (AUTH-R5). Stored alongside the JWT pair.
  Future<void> writeExpiry(DateTime expiresAt);

  /// The currently-cached access expiry (UTC), read synchronously for the
  /// proactive-refresh timer. `null` before the first [writeExpiry] / a token
  /// minted by an older server that did not return `expires_at`.
  DateTime? get expiresAtSync;
}

/// An in-memory [TokenStore] (tests, the boot stub, and a sane default before
/// the platform secure store is injected at bootstrap).
class InMemoryTokenStore implements TokenStore {
  InMemoryTokenStore({String? access, String? refresh})
      // ignore: prefer_initializing_formals
      : _access = access,
        // ignore: prefer_initializing_formals
        _refresh = refresh;

  String? _access;
  String? _refresh;
  DateTime? _expiresAt;

  @override
  String get accessSync => _access ?? '';

  @override
  Future<String?> readAccess() async => _access;

  @override
  Future<String?> readRefresh() async => _refresh;

  @override
  Future<void> write({required String access, required String refresh}) async {
    _access = access;
    _refresh = refresh;
  }

  @override
  Future<void> clear() async {
    _access = null;
    _refresh = null;
    _expiresAt = null;
  }

  @override
  Future<void> writeExpiry(DateTime expiresAt) async => _expiresAt = expiresAt;

  @override
  DateTime? get expiresAtSync => _expiresAt;
}
