// QCue D4: the audio-recorder seam for cloud voice STT. The voice controller records a clip via this
// facade, then uploads the bytes to /v1/transcribe. Kept as a pure interface (no plugin import) so host
// tests inject a fake and never touch the platform channel; the real impl lives in
// record_package_recorder.dart.
import 'dart:typed_data';

/// A finished recording: the encoded bytes plus their MIME type and duration.
class RecordedAudio {
  const RecordedAudio({
    required this.bytes,
    this.mime = 'audio/m4a',
    this.millis = 0,
  });

  final Uint8List bytes;
  final String mime;
  final int millis;
}

/// Push-to-talk audio capture. The voice controller calls [ensurePermission] →
/// [start] → [stop] (returns the clip) or [cancel] (discards it).
abstract interface class AudioRecorderFacade {
  /// Request/confirm the microphone permission. Returns false if denied.
  Future<bool> ensurePermission();

  /// Begin recording (m4a/AAC, 44.1 kHz mono). Throws on a hard failure.
  Future<void> start();

  /// Stop and return the recorded clip, or null if nothing was captured.
  Future<RecordedAudio?> stop();

  /// Abort recording and discard any captured audio (idempotent).
  Future<void> cancel();
}
