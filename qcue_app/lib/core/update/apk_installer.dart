// AU-R21 — apply a full update. iOS opens the App Store (Apple installs); Android downloads the APK
// (authenticated, via the JWT-gated proxy) and fires the OS install intent (the mandatory one-tap
// install — Android security forbids a silent install for a sideloaded app).
import 'dart:io';
import 'package:crypto/crypto.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;
import 'package:path_provider/path_provider.dart';
import 'package:url_launcher/url_launcher.dart';
import '../models/app_release_manifest.dart';
import '../native/installer/installer_channel.dart';
import '../net/api_client_provider.dart';

/// Pure decision helper (tested): which target a platform updates against.
String? updateTargetFor({required String platform, String? appStoreUrl, String? apkPath}) =>
    platform == 'ios' ? appStoreUrl : apkPath;

/// AU-R21 SECURITY (pure, tested): does a downloaded APK pass its integrity check?
/// Returns true when [expectedSha256] is null/empty (the server has not published a
/// digest yet — forward-compatible; TLS + the OS same-signer check still apply), OR
/// when the SHA-256 of [bytes] matches [expectedSha256] (case-insensitive hex). A
/// mismatch returns false and the caller MUST abort the install — a tampered/corrupt
/// download (compromised proxy/backend, or a TLS-stripping MITM) is never installed.
bool apkDigestOk({required List<int> bytes, String? expectedSha256}) {
  final expected = expectedSha256?.trim().toLowerCase();
  if (expected == null || expected.isEmpty) return true;
  final actual = sha256.convert(bytes).toString().toLowerCase();
  return actual == expected;
}

Future<void> applyFullUpdate(WidgetRef ref, AppReleaseManifest m, BuildContext context) async {
  if (Platform.isIOS) {
    final url = m.iosAppStoreUrl;
    if (url != null) {
      await launchUrl(Uri.parse(url), mode: LaunchMode.externalApplication);
    }
    return;
  }
  // Android: download the APK with the bearer token to app storage, then install.
  final path = m.androidApkPath;
  if (path == null) return;
  final cfg = ref.read(qcueConfigProvider);
  final token = ref.read(tokenStoreProvider).accessSync;
  final dir = await getTemporaryDirectory();
  final file = File('${dir.path}/qcue-update-${m.latestBuild}.apk');
  final resp = await http.get(cfg.uri(path), headers: {'Authorization': 'Bearer $token'});
  if (resp.statusCode != 200) {
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text("Couldn't download the update — try again.")),
      );
    }
    return;
  }
  // SECURITY (AU-R21): verify the download's SHA-256 against the release manifest BEFORE staging or
  // installing it. The bytes come over our JWT-authenticated TLS proxy and Android re-checks the signing
  // cert at install time, but this is cheap defense in depth: it stops a tampered APK from a compromised
  // proxy/backend (or a TLS-stripping MITM) from ever reaching the installer. Skips (returns ok) until
  // the server publishes `android_apk_sha256`, so it is safe to ship ahead of the backend.
  if (!apkDigestOk(bytes: resp.bodyBytes, expectedSha256: m.androidApkSha256)) {
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Update failed an integrity check — not installed.')),
      );
    }
    return;
  }
  await file.writeAsBytes(resp.bodyBytes);
  try {
    await ref.read(installerChannelProvider).installApk(file.path);
  } catch (_) {
    // A failed install-intent (no installer activity / FileProvider misconfig) must NOT strand the user
    // — especially on the blocking UpdateRequiredScreen, the one place they cannot proceed. Surface it.
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text("Couldn't start the install — try again.")),
      );
    }
  }
}
