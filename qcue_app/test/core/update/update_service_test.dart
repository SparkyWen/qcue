import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:qcue_app/core/models/app_release_manifest.dart';
import 'package:qcue_app/core/native/codepush/code_push_facade.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/update/update_service.dart';
import 'package:qcue_app/core/update/update_state.dart';

// StubApiClient exposes only factory constructors, so a fake `extends Fake implements QcueApiClient`
// and overrides just the one method UpdateService calls (any other call throws — none are made).
class _FakeApi extends Fake implements QcueApiClient {
  _FakeApi(this._m);
  final AppReleaseManifest _m;
  @override
  Future<AppReleaseManifest> fetchReleaseManifest(String platform) async => _m;
}

ProviderContainer _container({
  required AppReleaseManifest manifest,
  required int currentBuild,
  CodePushStatus patch = const CodePushStatus(),
}) {
  return ProviderContainer(overrides: [
    apiClientProvider.overrideWithValue(_FakeApi(manifest)),
    currentBuildProvider.overrideWithValue(currentBuild),
    codePushFacadeProvider.overrideWithValue(
        StubCodePushFacade(currentPatch: patch.currentPatch, updateReady: patch.updateReady)),
    updatePlatformProvider.overrideWithValue('android'),
  ]);
}

AppReleaseManifest _manifest({required int latest, required int min}) => AppReleaseManifest(
      platform: 'android',
      latestBuild: latest,
      latestVersion: '1.0.$latest',
      minSupportedBuild: min,
      changelog: 'notes',
      androidApkPath: '/v1/app/apk/$latest',
      iosAppStoreUrl: null,
      publishedAt: 't',
    );

void main() {
  test('current >= latest, no patch ⇒ UpToDate', () async {
    final c = _container(manifest: _manifest(latest: 9, min: 5), currentBuild: 9);
    final s = await c.read(updateProvider.future);
    expect(s, isA<UpToDate>());
    c.dispose();
  });

  test('staged patch ⇒ PatchReady', () async {
    final c = _container(
        manifest: _manifest(latest: 9, min: 5),
        currentBuild: 9,
        patch: const CodePushStatus(currentPatch: 2, updateReady: true));
    final s = await c.read(updateProvider.future);
    expect(s, isA<PatchReady>());
    c.dispose();
  });

  test('latest > current ⇒ FullUpdateAvailable', () async {
    final c = _container(manifest: _manifest(latest: 12, min: 5), currentBuild: 9);
    final s = await c.read(updateProvider.future);
    expect(s, isA<FullUpdateAvailable>());
    c.dispose();
  });

  test('current < min_supported ⇒ UpdateRequired (force gate)', () async {
    final c = _container(manifest: _manifest(latest: 12, min: 11), currentBuild: 9);
    final s = await c.read(updateProvider.future);
    expect(s, isA<UpdateRequired>());
    expect(c.read(updateGateProvider), isTrue);
    c.dispose();
  });

  test('force gate takes precedence over a staged patch', () async {
    final c = _container(
        manifest: _manifest(latest: 12, min: 11),
        currentBuild: 9,
        patch: const CodePushStatus(currentPatch: 1, updateReady: true));
    final s = await c.read(updateProvider.future);
    expect(s, isA<UpdateRequired>());
    c.dispose();
  });
}
