import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/update/apk_installer.dart';

void main() {
  test('iOS uses the App Store url; Android uses the apk path', () {
    expect(updateTargetFor(platform: 'ios', appStoreUrl: 'itms://x', apkPath: null), 'itms://x');
    expect(updateTargetFor(platform: 'android', appStoreUrl: null, apkPath: '/v1/app/apk/10'),
        '/v1/app/apk/10');
  });

  group('apkDigestOk (AU-R21 SECURITY)', () {
    // sha256("abc") — a standard NIST test vector.
    const abc = [0x61, 0x62, 0x63];
    const abcSha = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad';

    test('no published digest → passes (forward-compatible with the server)', () {
      expect(apkDigestOk(bytes: abc, expectedSha256: null), isTrue);
      expect(apkDigestOk(bytes: abc, expectedSha256: ''), isTrue);
      expect(apkDigestOk(bytes: abc, expectedSha256: '   '), isTrue);
    });

    test('a matching SHA-256 passes (case-insensitive hex)', () {
      expect(apkDigestOk(bytes: abc, expectedSha256: abcSha), isTrue);
      expect(apkDigestOk(bytes: abc, expectedSha256: abcSha.toUpperCase()), isTrue);
      expect(apkDigestOk(bytes: abc, expectedSha256: '  $abcSha  '), isTrue);
    });

    test('a mismatching SHA-256 fails → caller aborts the install', () {
      expect(apkDigestOk(bytes: abc, expectedSha256: 'deadbeef'), isFalse);
      expect(apkDigestOk(bytes: const [0x00, 0x01], expectedSha256: abcSha), isFalse);
    });
  });
}
