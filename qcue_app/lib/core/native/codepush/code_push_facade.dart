// AU-R14: a read-only Shorebird status facade. The Shorebird ENGINE auto-checks/downloads/applies
// patches (无感, AU-D6); this surface only *reports* state for the Settings update section. Wrapped
// behind an interface so tests (and the keyless stub build, which has no Shorebird engine) inject a
// scripted value — `flutter test`/`QCUE_STUB` builds are not Shorebird releases, so the real updater
// reports `isAvailable == false` there and the app must still work.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shorebird_code_push/shorebird_code_push.dart';

class CodePushStatus {
  const CodePushStatus({this.currentPatch, this.updateReady = false});

  /// The applied patch number, or null on a clean release / when Shorebird is unavailable.
  final int? currentPatch;

  /// A downloaded patch is staged and will apply on the next launch (UpdateStatus.restartRequired).
  final bool updateReady;
}

abstract interface class CodePushFacade {
  Future<CodePushStatus> status();
}

/// Scripted stub for tests + the keyless `QCUE_STUB` build (no Shorebird engine present).
class StubCodePushFacade implements CodePushFacade {
  const StubCodePushFacade({this.currentPatch, this.updateReady = false});
  final int? currentPatch;
  final bool updateReady;
  @override
  Future<CodePushStatus> status() async =>
      CodePushStatus(currentPatch: currentPatch, updateReady: updateReady);
}

/// Real facade over `package:shorebird_code_push` 2.0.4. Construction never throws; in a non-Shorebird
/// build (debug / not built with `shorebird release`) `isAvailable` is false and every call resolves to
/// a benign "no patch, not ready" status. `readCurrentPatch` can throw [ReadPatchException]; the guard
/// turns any failure into the benign status so the update surface never crashes the app.
class ShorebirdCodePushFacade implements CodePushFacade {
  ShorebirdCodePushFacade() : _updater = ShorebirdUpdater();
  final ShorebirdUpdater _updater;

  @override
  Future<CodePushStatus> status() async {
    try {
      if (!_updater.isAvailable) return const CodePushStatus();
      final patch = await _updater.readCurrentPatch();
      final outcome = await _updater.checkForUpdate();
      return CodePushStatus(
        currentPatch: patch?.number,
        updateReady: outcome == UpdateStatus.restartRequired,
      );
    } catch (_) {
      return const CodePushStatus();
    }
  }
}

/// The active Shorebird status facade. Stub by default (tests + keyless build); overridden at
/// bootstrap on a real device with `ShorebirdCodePushFacade()`.
final codePushFacadeProvider = Provider<CodePushFacade>((_) => const StubCodePushFacade());

/// The running build number (pubspec `+BUILD`). 0 by default; overridden at bootstrap from
/// `PackageInfo.fromPlatform().buildNumber`. Tests override with a literal.
final currentBuildProvider = Provider<int>((_) => 0);
