// QCue S4/S5: the push-to-talk seam. The S4 foundation shipped a stub returning
// a canned transcript; S5 adds the real, native-backed [NativeVoiceCapture
// controller] (on-device STT with a deterministic cloud fallback — D4). The
// provider defaults to the stub so host widget tests stay deterministic with no
// platform channel; the bootstrap overrides it with the native controller on
// device (see [nativeVoiceCaptureController]).
import 'package:flutter_riverpod/flutter_riverpod.dart';

abstract interface class VoiceCaptureController {
  /// Begin a tap-to-stop capture. Resolves with the final transcript when [stop]
  /// is called (or when the recognizer finalizes / a safety cap elapses). Returns
  /// '' on denied permission or a cancelled/empty capture.
  Future<String> capture();

  /// Stop the in-flight capture and let it finalize (idempotent; no-op when
  /// idle). This is the user-driven "tap to stop" — on iOS it triggers the
  /// recognizer's endAudio so a final is actually produced.
  Future<void> stop();

  /// Abort the in-flight capture with no transcript (idempotent; no-op when
  /// idle). Used on dispose so a left-open mic never hangs.
  Future<void> cancel();
}

/// Canned transcript so the mic path is testable without real STT.
class StubVoiceCaptureController implements VoiceCaptureController {
  const StubVoiceCaptureController(
      [this.transcript = 'Voice note: revisit the recall trade-off.']);
  final String transcript;

  @override
  Future<String> capture() async => transcript;

  @override
  Future<void> stop() async {}

  @override
  Future<void> cancel() async {}
}

/// The push-to-talk controller the Capture field uses. Overridden at bootstrap
/// (real native STT + cloud fallback) on device; the stub here keeps host tests
/// deterministic.
final voiceCaptureProvider = Provider<VoiceCaptureController>(
    (_) => const StubVoiceCaptureController());
