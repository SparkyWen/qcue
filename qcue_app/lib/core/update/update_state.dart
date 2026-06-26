// AU-R16 — the sealed update state every update surface renders. Precedence (highest first):
// UpdateRequired > FullUpdateAvailable > PatchReady > UpToDate.
import '../models/app_release_manifest.dart';

sealed class UpdateState {
  const UpdateState();
}

class UpToDate extends UpdateState {
  const UpToDate();
}

/// A Shorebird patch is downloaded and applies on next launch (informational, AU-D6).
class PatchReady extends UpdateState {
  const PatchReady(this.patchNumber);
  final int? patchNumber;
}

/// A newer full release exists (native/engine) — non-blocking nudge.
class FullUpdateAvailable extends UpdateState {
  const FullUpdateAvailable(this.manifest);
  final AppReleaseManifest manifest;
}

/// The running build is below `min_supported_build` — blocking gate (AU-D3).
class UpdateRequired extends UpdateState {
  const UpdateRequired(this.manifest);
  final AppReleaseManifest manifest;
}
