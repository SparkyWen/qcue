// QCue cloud-sync fix (Task 4): the runtime server-URL resolution + store.
// Pins:
//   - a valid runtime override BEATS the build-time default (point at a deployed
//     server without a rebuild);
//   - an INVALID override is rejected and falls through to the next source;
//   - the build-time default beats the local-bind fallback;
//   - ServerUrlStore round-trips the value through SharedPreferences.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_config.dart';
import 'package:qcue_app/core/net/server_url_store.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  group('QcueConfig.resolveBaseUrl', () {
    test('a valid runtime override beats the build-time default', () {
      final url = QcueConfig.resolveBaseUrl(
        runtimeOverride: 'https://api.example.com',
        buildTimeDefault: 'http://127.0.0.1:8787',
      );
      expect(url, 'https://api.example.com');
    });

    test('a trailing slash is stripped from the resolved URL', () {
      final url = QcueConfig.resolveBaseUrl(
        runtimeOverride: 'https://api.example.com/',
      );
      expect(url, 'https://api.example.com');
    });

    test('an invalid override is rejected and falls through to the default', () {
      final url = QcueConfig.resolveBaseUrl(
        runtimeOverride: 'not a url',
        buildTimeDefault: 'https://staging.example.com',
      );
      expect(url, 'https://staging.example.com');
    });

    test('a non-http(s) scheme is rejected', () {
      expect(QcueConfig.isValidBaseUrl('ws://example.com'), isFalse);
      expect(QcueConfig.isValidBaseUrl('ftp://example.com'), isFalse);
      expect(QcueConfig.isValidBaseUrl('example.com'), isFalse);
      expect(QcueConfig.isValidBaseUrl(''), isFalse);
      expect(QcueConfig.isValidBaseUrl(null), isFalse);
    });

    test('http and https are both valid', () {
      expect(QcueConfig.isValidBaseUrl('http://127.0.0.1:8787'), isTrue);
      expect(QcueConfig.isValidBaseUrl('https://api.qcue.app'), isTrue);
    });

    // SECURITY (transport): cleartext http:// is allowed only for loopback /
    // emulator dev hosts; any remote host must be https. The REST + ws:// clients
    // run over dart:io sockets that ignore Android NSC / iOS ATS, so this Dart
    // guard is the sole defense against a cleartext bearer JWT / BYOK key leak.
    test('http:// is allowed only for loopback / emulator dev hosts', () {
      expect(QcueConfig.isValidBaseUrl('http://localhost:9200'), isTrue);
      expect(QcueConfig.isValidBaseUrl('http://127.0.0.1:9200'), isTrue);
      expect(QcueConfig.isValidBaseUrl('http://10.0.2.2:9200'), isTrue);
      expect(QcueConfig.isValidBaseUrl('http://[::1]:9200'), isTrue);
    });

    test('http:// to a remote host is rejected (must be https)', () {
      expect(QcueConfig.isValidBaseUrl('http://app.qcue.cn'), isFalse);
      expect(QcueConfig.isValidBaseUrl('http://attacker.example'), isFalse);
      expect(QcueConfig.isValidBaseUrl('http://10.0.2.2.attacker.com'), isFalse);
      // https to those same hosts stays valid.
      expect(QcueConfig.isValidBaseUrl('https://app.qcue.cn'), isTrue);
    });

    test('a remote http:// override is rejected and falls through to https default', () {
      final url = QcueConfig.resolveBaseUrl(
        runtimeOverride: 'http://attacker.example',
        buildTimeDefault: 'https://app.qcue.cn',
      );
      expect(url, 'https://app.qcue.cn');
    });

    test('the build-time default beats the local-bind fallback', () {
      final url = QcueConfig.resolveBaseUrl(
        runtimeOverride: null,
        buildTimeDefault: 'https://build.example.com',
      );
      expect(url, 'https://build.example.com');
    });

    test('falls back to defaultBaseUrl when everything is invalid', () {
      final url = QcueConfig.resolveBaseUrl(
        runtimeOverride: 'garbage',
        buildTimeDefault: 'also garbage',
      );
      expect(url, QcueConfig.defaultBaseUrl);
    });
  });

  group('ServerUrlStore', () {
    setUp(() => SharedPreferences.setMockInitialValues({}));

    test('round-trips the override through SharedPreferences', () async {
      final prefs = await SharedPreferences.getInstance();
      final store = ServerUrlStore(prefs);
      expect(store.read(), isNull);

      await store.write('https://api.example.com');
      expect(store.read(), 'https://api.example.com');

      // A new store over the same prefs sees the persisted value.
      final reloaded = ServerUrlStore(await SharedPreferences.getInstance());
      expect(reloaded.read(), 'https://api.example.com');
    });

    test('writing an empty string clears the override', () async {
      final prefs = await SharedPreferences.getInstance();
      final store = ServerUrlStore(prefs);
      await store.write('https://api.example.com');
      await store.write('   ');
      expect(store.read(), isNull);
    });

    test('clear removes the override (falls back to default)', () async {
      final prefs = await SharedPreferences.getInstance();
      final store = ServerUrlStore(prefs);
      await store.write('https://api.example.com');
      await store.clear();
      expect(store.read(), isNull);
    });
  });
}
