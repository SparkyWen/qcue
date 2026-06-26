// AU-R21 — the Android APK install intent, behind a versioned method-channel facade. iOS never calls
// this (App Store installs). schemaVersion:1; raw OS exceptions wrap to the closed NativeError set.
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../channels.dart'; // NativeError + nativeErrorFrom + channel conventions

abstract interface class InstallerChannel {
  Future<void> installApk(String filePath);
}

class MethodInstallerChannel implements InstallerChannel {
  const MethodInstallerChannel();
  static const _ch = MethodChannel('qcue/installer');
  @override
  Future<void> installApk(String filePath) async {
    try {
      await _ch.invokeMethod<void>('installApk', {'schemaVersion': 1, 'filePath': filePath});
    } on PlatformException catch (e) {
      throw nativeErrorFrom(e); // closed NativeError set — raw OS exceptions never leak
    }
  }
}

class StubInstallerChannel implements InstallerChannel {
  const StubInstallerChannel();
  @override
  Future<void> installApk(String filePath) async {}
}

/// Stub by default (tests/keyless); overridden at bootstrap on Android with [MethodInstallerChannel].
final installerChannelProvider = Provider<InstallerChannel>((_) => const StubInstallerChannel());
