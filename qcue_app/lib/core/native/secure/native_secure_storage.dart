// QCue S5-R24/R26/D9: the device-cached BYOK key store backed by the OS secure
// element (iOS Keychain / Android Keystore-wrapped EncryptedSharedPreferences),
// reached over MethodChannel('qcue/secure'). The Dart layer holds NO plaintext
// key state and NEVER logs a value — it carries opaque (wrapped) blobs by `key`.
// An optional biometric gate (S5-R26) is applied on read; a denied/failed
// biometric fails CLOSED (returns null), never surfacing the blob unauthenticated.
import 'package:flutter/services.dart';
import '../../secure/secure_storage.dart';
import '../channels.dart';

/// The real, native-backed [SecureStorage]. Picked at bootstrap on device; host
/// tests use [NullSecureStorage] or a fake. Construct with [requireBiometric]
/// true to gate reads behind Face/Touch ID / BiometricPrompt (S5-R26).
class NativeSecureStorage implements SecureStorage {
  const NativeSecureStorage({
    this.requireBiometric = false,
    this._channel = const MethodChannel(QcueChannels.secure),
  });

  final bool requireBiometric;
  final MethodChannel _channel;

  @override
  Future<void> write(String key, String value) async {
    // Write does NOT prompt (S5-R26) so the key can be cached when the vault
    // syncs; the value is an opaque wrapped blob produced server/Rust-side.
    // SECURITY: forward [requireBiometric] so the native side binds an OS-ENFORCED
    // access-control ACL on biometric items (the BYOK vault, requireBiometric:true)
    // — and ONLY those. The JWT session store uses a separate instance with
    // requireBiometric:false (see main.dart), so its items stay plain and are read
    // prompt-free on every launch; the gate is no longer app-code-only on iOS.
    await _channel.invokeMethod<void>(
      'write',
      QcueChannels.envelope({
        'key': key,
        'value': value,
        'requireBiometric': requireBiometric,
      }),
    );
  }

  /// Read the wrapped blob for [key]. A biometric-gated read shows [reason] in
  /// the OS prompt; a denial fails closed (returns null), never the blob.
  @override
  Future<String?> read(String key, {String? reason}) async {
    try {
      return await _channel.invokeMethod<String>(
        'read',
        QcueChannels.envelope({
          'key': key,
          'requireBiometric': requireBiometric,
          if (reason != null) 'reason': reason,
        }),
      );
    } on PlatformException catch (e) {
      final err = nativeErrorFrom(e);
      // Fail closed: a denied/cancelled biometric (or any auth failure) yields
      // null — the on-device call is refused, the blob is NEVER returned
      // unauthenticated (S5-R26). Other OS errors also degrade to null.
      if (err.kind == NativeErrorKind.permissionDenied ||
          err.kind == NativeErrorKind.cancelled) {
        return null;
      }
      return null;
    }
  }

  @override
  Future<void> delete(String key) async {
    await _channel.invokeMethod<void>(
      'delete',
      QcueChannels.envelope({'key': key}),
    );
  }

  /// True if a wrapped blob is stored for [key] (no biometric prompt).
  Future<bool> containsKey(String key) async {
    final r = await _channel.invokeMethod<bool>(
      'containsKey',
      QcueChannels.envelope({'key': key}),
    );
    return r ?? false;
  }

  /// Whether the device has an enrolled biometric (for the gate UI).
  Future<bool> biometricAvailable() async {
    final r = await _channel.invokeMethod<bool>(
      'biometricAvailable',
      QcueChannels.envelope(),
    );
    return r ?? false;
  }

  @override
  String toString() => 'NativeSecureStorage(requireBiometric: $requireBiometric)';
}
