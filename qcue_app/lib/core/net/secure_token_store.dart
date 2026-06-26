// QCue cloud-sync fix (Task 3): a DURABLE [TokenStore] so a signed-in session
// survives an app restart. The empty [InMemoryTokenStore] is the root cause of
// the "server not connected" bug — with no login + no persisted token, every
// REST call sent `Authorization: Bearer ` (empty) and the server 401'd.
//
// [SecureTokenStore] is backed by the device secure element (iOS Keychain /
// Android Keystore-wrapped EncryptedSharedPreferences) through the existing
// [SecureStorage] seam ([NativeSecureStorage] on device, [NullSecureStorage] in
// host tests). The JWT pair is the ONLY thing it holds — never a provider
// secret. An in-memory mirror is hydrated at construct time ([load]) so
// [accessSync] stays synchronous for the SSE/WSS `?token=` callback.
import 'dart:async';

import '../secure/secure_storage.dart';
import 'qcue_config.dart';

/// A durable [TokenStore] over the OS secure store. Construct via [load] so the
/// in-memory mirror is hydrated from disk before first use (keeping [accessSync]
/// synchronous). Writes persist to the secure store AND the mirror.
class SecureTokenStore implements TokenStore {
  SecureTokenStore._(
    this._secure, {
    String? access,
    String? refresh,
    DateTime? expiresAt,
    this.hadReadError = false,
  })
      // ignore: prefer_initializing_formals
      : _access = access,
        // ignore: prefer_initializing_formals
        _refresh = refresh,
        // ignore: prefer_initializing_formals
        _expiresAt = expiresAt;

  /// The secure-store keys for the persisted JWT pair. Distinct from the BYOK
  /// vault keys (those carry provider blobs); these carry only the session JWT.
  static const accessKey = 'qcue.session.access_jwt';
  static const refreshKey = 'qcue.session.refresh_jwt';
  static const expiresKey = 'qcue.session.expires_at';

  final SecureStorage _secure;
  String? _access;
  String? _refresh;
  DateTime? _expiresAt;

  /// True when the hydrating read threw (locked Keychain / missing native
  /// module) rather than finding an empty store. AUTH-R7: a transient read
  /// failure must NOT be treated as "signed out" — the caller keeps the session
  /// and must not wipe the durable backing.
  final bool hadReadError;

  /// Hydrate from the secure store, returning a store whose mirror already holds
  /// any persisted session — so a restart resumes signed-in without an await on
  /// the hot path. A read failure degrades to an empty (signed-out) store.
  static Future<SecureTokenStore> load(SecureStorage secure) async {
    String? access;
    String? refresh;
    String? expiresRaw;
    var readError = false;
    try {
      access = await secure.read(accessKey);
      refresh = await secure.read(refreshKey);
      expiresRaw = await secure.read(expiresKey);
    } catch (_) {
      // AUTH-R7: a secure-store read failure (locked device, missing native
      // module under tests) is TRANSIENT, not an empty store. Flag it so the
      // caller does not wipe a session that may still be valid.
      access = null;
      refresh = null;
      expiresRaw = null;
      readError = true;
    }
    return SecureTokenStore._(
      secure,
      access: (access != null && access.isNotEmpty) ? access : null,
      refresh: (refresh != null && refresh.isNotEmpty) ? refresh : null,
      expiresAt: (expiresRaw != null && expiresRaw.isNotEmpty)
          ? DateTime.tryParse(expiresRaw)
          : null,
      hadReadError: readError,
    );
  }

  @override
  String get accessSync => _access ?? '';

  @override
  Future<String?> readAccess() async => _access;

  @override
  Future<String?> readRefresh() async => _refresh;

  @override
  Future<void> write({required String access, required String refresh}) async {
    // Mirror first (so accessSync is correct even if the durable write is slow),
    // then persist. Both keys are written so a restart rehydrates the full pair.
    _access = access;
    _refresh = refresh;
    await _secure.write(accessKey, access);
    await _secure.write(refreshKey, refresh);
  }

  @override
  Future<void> clear() async {
    _access = null;
    _refresh = null;
    _expiresAt = null;
    await _secure.delete(accessKey);
    await _secure.delete(refreshKey);
    await _secure.delete(expiresKey);
  }

  @override
  Future<void> writeExpiry(DateTime expiresAt) async {
    _expiresAt = expiresAt;
    await _secure.write(expiresKey, expiresAt.toUtc().toIso8601String());
  }

  @override
  DateTime? get expiresAtSync => _expiresAt;
}
