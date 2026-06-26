// QCue S5-D9: the device-cached BYOK secure-store seam as a Riverpod provider.
// Defaults to the inert [NullSecureStorage] so host tests store nothing; the
// bootstrap overrides it with the native, Keychain/Keystore-backed
// [NativeSecureStorage] on device. The BYOK key flow reads this one provider.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'secure_storage.dart';

/// The active device secure store. Overridden at app bootstrap (real native
/// store) and inert (Null) under tests.
final secureStorageProvider =
    Provider<SecureStorage>((_) => const NullSecureStorage());
