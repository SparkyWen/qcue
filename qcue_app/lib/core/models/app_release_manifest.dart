// AU-R7 — Dart mirror of app_server_protocol::v1::AppReleaseManifest. Hand-written (like the other
// wire models in protocol_models.dart). Build numbers are integers; the app compares them against
// `currentBuildProvider`.
import 'package:flutter/foundation.dart';

@immutable
class AppReleaseManifest {
  const AppReleaseManifest({
    required this.platform,
    required this.latestBuild,
    required this.latestVersion,
    required this.minSupportedBuild,
    required this.changelog,
    required this.androidApkPath,
    this.androidApkSha256,
    required this.iosAppStoreUrl,
    required this.publishedAt,
  });

  final String platform;
  final int latestBuild;
  final String latestVersion;
  final int minSupportedBuild;
  final String changelog;
  final String? androidApkPath;

  /// AU-R21 SECURITY: the expected SHA-256 (lowercase hex) of the Android APK at
  /// [androidApkPath]. When the server publishes it, the in-app updater verifies
  /// the downloaded bytes against it BEFORE invoking the OS installer (defense in
  /// depth atop TLS + the installer's same-signer check). `null` ⇒ the server has
  /// not published a digest yet → the updater proceeds (forward-compatible).
  final String? androidApkSha256;

  final String? iosAppStoreUrl;
  final String publishedAt;

  factory AppReleaseManifest.fromJson(Map<String, dynamic> j) => AppReleaseManifest(
        platform: j['platform'] as String? ?? 'android',
        latestBuild: (j['latest_build'] as num?)?.toInt() ?? 0,
        latestVersion: j['latest_version'] as String? ?? '',
        minSupportedBuild: (j['min_supported_build'] as num?)?.toInt() ?? 0,
        changelog: j['changelog'] as String? ?? '',
        androidApkPath: j['android_apk_path'] as String?,
        androidApkSha256: j['android_apk_sha256'] as String?,
        iosAppStoreUrl: j['ios_app_store_url'] as String?,
        publishedAt: j['published_at'] as String? ?? '',
      );

  /// The benign "no update info" manifest (used when offline / endpoint degraded).
  static const none = AppReleaseManifest(
    platform: 'android',
    latestBuild: 0,
    latestVersion: '',
    minSupportedBuild: 0,
    changelog: '',
    androidApkPath: null,
    androidApkSha256: null,
    iosAppStoreUrl: null,
    publishedAt: '',
  );
}
