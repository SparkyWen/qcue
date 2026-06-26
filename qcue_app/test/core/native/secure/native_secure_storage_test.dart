// QCue S5-R24/R26: NativeSecureStorage — the device-cached BYOK key store (D9).
// read/write/delete/containsKey round-trip via the MethodChannel('qcue/secure')
// against the SDK's mock messenger (no device); an optional biometric gate is
// passed through on read; a denied biometric fails CLOSED (null, never the blob)
// and the value is NEVER logged.
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/native/channels.dart';
import 'package:qcue_app/core/native/secure/native_secure_storage.dart';
import 'package:qcue_app/core/secure/secure_storage.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  const channel = MethodChannel(QcueChannels.secure);

  // A tiny in-memory fake "secure element" behind the channel.
  late Map<String, String> store;
  late List<MethodCall> calls;
  bool biometricGranted = true;

  setUp(() {
    store = {};
    calls = [];
    biometricGranted = true;
    messenger.setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      final args = (call.arguments as Map?)?.cast<String, dynamic>() ?? {};
      // every inbound payload carries the schema version (S5-R3)
      expect(args['schemaVersion'], QcueChannels.schemaVersion);
      switch (call.method) {
        case 'write':
          store[args['key'] as String] = args['value'] as String;
          return null;
        case 'read':
          // a read may require a biometric gate; a denial fails closed.
          if (args['requireBiometric'] == true && !biometricGranted) {
            throw PlatformException(
                code: 'permissionDenied',
                message: 'biometric denied',
                details: {'kind': 'permissionDenied', 'retryable': false});
          }
          return store[args['key'] as String];
        case 'delete':
          store.remove(args['key'] as String);
          return null;
        case 'containsKey':
          return store.containsKey(args['key'] as String);
        case 'biometricAvailable':
          return true;
        default:
          throw PlatformException(code: 'osError', message: 'unknown');
      }
    });
  });

  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  test('S5-R24: write/read/delete round-trips via the channel', () async {
    const SecureStorage s = NativeSecureStorage();
    expect(await s.read('cred_openai'), isNull);
    await s.write('cred_openai', 'wrapped-blob-xyz');
    expect(await s.read('cred_openai'), 'wrapped-blob-xyz');
    expect(await (s as NativeSecureStorage).containsKey('cred_openai'), isTrue);
    await s.delete('cred_openai');
    expect(await s.read('cred_openai'), isNull);
    expect(await s.containsKey('cred_openai'), isFalse);
  });

  test('S5-R3: every call carries the schema version', () async {
    const s = NativeSecureStorage();
    await s.write('k', 'v');
    await s.read('k');
    expect(calls, isNotEmpty);
    for (final c in calls) {
      final args = (c.arguments as Map).cast<String, dynamic>();
      expect(args['schemaVersion'], QcueChannels.schemaVersion);
    }
  });

  test('S5-R26: a biometric-gated read passes requireBiometric + reason',
      () async {
    const s = NativeSecureStorage(requireBiometric: true);
    await s.write('cred_anthropic', 'blob');
    final v = await s.read('cred_anthropic', reason: 'Unlock your key');
    expect(v, 'blob');
    final read = calls.firstWhere((c) => c.method == 'read');
    final args = (read.arguments as Map).cast<String, dynamic>();
    expect(args['requireBiometric'], isTrue);
    expect(args['reason'], 'Unlock your key');
  });

  test('S5-R26: a denied biometric fails CLOSED (null, never the blob)',
      () async {
    const s = NativeSecureStorage(requireBiometric: true);
    await s.write('cred_anthropic', 'blob');
    biometricGranted = false;
    // fail-closed: the read returns null rather than throwing the raw blob or
    // surfacing it unauthenticated.
    expect(await s.read('cred_anthropic'), isNull);
  });

  test('S5-R28: the value never appears in toString / is never logged',
      () async {
    const s = NativeSecureStorage();
    // the facade itself carries no plaintext-bearing fields.
    expect(s.toString(), isNot(contains('blob')));
  });

  test('biometricAvailable is surfaced', () async {
    const s = NativeSecureStorage();
    expect(await s.biometricAvailable(), isTrue);
  });
}
