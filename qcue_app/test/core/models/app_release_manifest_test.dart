import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/app_release_manifest.dart';

void main() {
  test('fromJson maps snake_case wire fields', () {
    final m = AppReleaseManifest.fromJson(const {
      'platform': 'android',
      'latest_build': 10,
      'latest_version': '1.0.4',
      'min_supported_build': 9,
      'changelog': 'Bug fixes.',
      'android_apk_path': '/v1/app/apk/10',
      'ios_app_store_url': null,
      'published_at': '2026-06-24T00:00:00Z',
    });
    expect(m.latestBuild, 10);
    expect(m.minSupportedBuild, 9);
    expect(m.androidApkPath, '/v1/app/apk/10');
    expect(m.iosAppStoreUrl, isNull);
  });

  test('none is the benign no-update manifest', () {
    expect(AppReleaseManifest.none.latestBuild, 0);
    expect(AppReleaseManifest.none.minSupportedBuild, 0);
  });
}
