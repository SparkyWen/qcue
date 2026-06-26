import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:qcue_app/core/models/app_release_manifest.dart';
import 'package:qcue_app/core/native/codepush/code_push_facade.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/core/update/update_service.dart';
import 'package:qcue_app/core/update/software_update_section.dart';

class _Api extends Fake implements QcueApiClient {
  _Api(this._m);
  final AppReleaseManifest _m;
  @override
  Future<AppReleaseManifest> fetchReleaseManifest(String platform) async => _m;
}

// updatePrefsStoreProvider defaults to an in-memory store, so the section renders with no extra override.
Widget _host(AppReleaseManifest manifest, int currentBuild) {
  return ProviderScope(
    overrides: [
      apiClientProvider.overrideWithValue(_Api(manifest)),
      currentBuildProvider.overrideWithValue(currentBuild),
      codePushFacadeProvider.overrideWithValue(const StubCodePushFacade()),
      updatePlatformProvider.overrideWithValue('android'),
    ],
    child: MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: const Scaffold(body: SoftwareUpdateSection()),
    ),
  );
}

void main() {
  testWidgets('shows "Up to date" + a Check button when current==latest', (tester) async {
    await tester.pumpWidget(_host(
      const AppReleaseManifest(
          platform: 'android',
          latestBuild: 9,
          latestVersion: '1.0.9',
          minSupportedBuild: 5,
          changelog: '',
          androidApkPath: null,
          iosAppStoreUrl: null,
          publishedAt: 't'),
      9,
    ));
    await tester.pumpAndSettle();
    expect(find.textContaining('Up to date'), findsOneWidget);
    expect(find.byKey(const ValueKey('check-for-updates')), findsOneWidget);
    expect(find.byKey(const ValueKey('auto-update-switch')), findsOneWidget);
  });

  testWidgets('shows the nudge CTA when a full update is available', (tester) async {
    await tester.pumpWidget(_host(
      const AppReleaseManifest(
          platform: 'android',
          latestBuild: 12,
          latestVersion: '1.0.12',
          minSupportedBuild: 5,
          changelog: 'Faster recall',
          androidApkPath: '/v1/app/apk/12',
          iosAppStoreUrl: null,
          publishedAt: 't'),
      9,
    ));
    await tester.pumpAndSettle();
    expect(find.text('New version available'), findsOneWidget);
    expect(find.byKey(const ValueKey('update-cta')), findsOneWidget);
  });
}
