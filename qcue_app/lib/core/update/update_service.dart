// AU-R16/R17 — merges Shorebird patch status + the release manifest + the running build into one
// UpdateState. No network beyond the single seam; safe under the keyless stub (manifest = none).
import 'dart:io' show Platform;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../native/codepush/code_push_facade.dart';
import '../net/api_client_provider.dart';
import 'update_prefs_store.dart';
import 'update_state.dart';

/// "ios" on iOS, else "android". Overridable in tests.
final updatePlatformProvider = Provider<String>((_) => Platform.isIOS ? 'ios' : 'android');

class UpdateNotifier extends AsyncNotifier<UpdateState> {
  @override
  Future<UpdateState> build() => _evaluate();

  Future<UpdateState> _evaluate() async {
    final platform = ref.read(updatePlatformProvider);
    final currentBuild = ref.read(currentBuildProvider);
    final manifest = await ref.read(apiClientProvider).fetchReleaseManifest(platform);
    final patch = await ref.read(codePushFacadeProvider).status();

    // Precedence: force-gate first, then a newer full release, then a staged seamless patch.
    if (manifest.minSupportedBuild > currentBuild) {
      return UpdateRequired(manifest);
    }
    if (manifest.latestBuild > currentBuild) {
      return FullUpdateAvailable(manifest);
    }
    if (patch.updateReady) {
      return PatchReady(patch.currentPatch);
    }
    return const UpToDate();
  }

  /// Manual "Check for updates" (AU-R17).
  Future<void> checkNow() async {
    state = const AsyncLoading();
    state = await AsyncValue.guard(_evaluate);
  }
}

final updateProvider =
    AsyncNotifierProvider<UpdateNotifier, UpdateState>(UpdateNotifier.new);

/// AU-R19 — true iff a forced update is required (drives the router gate). False while loading/UpToDate.
final updateGateProvider = Provider<bool>((ref) {
  final s = ref.watch(updateProvider);
  return s.maybeWhen(data: (v) => v is UpdateRequired, orElse: () => false);
});

/// AU-R20 — the auto-check toggle state, backed by the device-local store. Governs the manifest
/// auto-check + nudge ONLY (never Shorebird OTA, never the force gate).
class AutoCheckNotifier extends Notifier<bool> {
  @override
  bool build() => ref.watch(updatePrefsStoreProvider).autoCheckEnabled;
  Future<void> set(bool on) async {
    await ref.read(updatePrefsStoreProvider).setAutoCheck(on);
    state = on;
  }
}

final autoCheckProvider = NotifierProvider<AutoCheckNotifier, bool>(AutoCheckNotifier.new);
