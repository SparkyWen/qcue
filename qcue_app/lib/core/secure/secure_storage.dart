// QCue S4-R46: the platform-channel seam to Keychain/Keystore (S5). The Dart
// layer NEVER holds a provider key in plaintext — only an opaque handle and the
// masked last-4 hint. The real platform implementation lands with S5 native
// modules; this is the interface the BYOK flow depends on.
abstract interface class SecureStorage {
  Future<void> write(String key, String value);
  Future<String?> read(String key);
  Future<void> delete(String key);
}

/// An inert no-op secure store (boot default; overridden by the real platform
/// store at bootstrap). It deliberately stores nothing.
class NullSecureStorage implements SecureStorage {
  const NullSecureStorage();
  @override
  Future<void> write(String key, String value) async {}
  @override
  Future<String?> read(String key) async => null;
  @override
  Future<void> delete(String key) async {}
}
