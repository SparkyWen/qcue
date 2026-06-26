// QCue D4: the cloud-primary push-to-talk controller. Tap-to-start records an audio clip via the
// AudioRecorderFacade; tap-to-stop ends the take, uploads the bytes to the cloud STT
// (/v1/transcribe → OpenAI gpt-4o-transcribe), and resolves capture() with the transcript. The
// Capture field then drops that text into the EDITABLE compose field for review — it is NEVER
// auto-committed. Permission is gated up front: a denied mic permission resolves to '' (the UI shows
// a consent/permission message) and audio is never recorded or uploaded without it. A transcription
// failure (no key / provider error / offline) is PROPAGATED as a TranscribeException so the field can
// surface the REAL reason; permission-denied / empty-take / cancel still resolve to '' (their own meaning).
import 'dart:async';

import '../../../core/native/audio/audio_recorder.dart';
import 'voice_capture_controller.dart';

/// Cloud STT signature: upload the recorded clip and return the transcript. The device-cached BYOK
/// key (D9) and the wire live behind the api client; injected so the controller is testable with no net.
typedef TranscribeCloud = Future<String> Function({
  required List<int> audio,
  String? language,
});

class NativeVoiceCaptureController implements VoiceCaptureController {
  NativeVoiceCaptureController({
    required this.recorder,
    required this.transcribeCloud,
    this.language,
    // Safety cap: tap-to-stop ends the take; this only fires if the user never does.
    this.timeout = const Duration(minutes: 5),
  });

  final AudioRecorderFacade recorder;
  final TranscribeCloud transcribeCloud;

  /// Optional ISO-639-1 language hint for the cloud STT (null = auto-detect).
  final String? language;
  final Duration timeout;

  Completer<String>? _inflight;
  bool _cancelled = false;
  bool _finishing = false;
  Timer? _safety;

  @override
  Future<String> capture() async {
    _cancelled = false;
    _finishing = false;

    // 1) Permission gate: a denied mic permission must not record or upload audio.
    final granted = await recorder.ensurePermission();
    if (!granted) return '';

    // 2) Start recording. A start failure resolves empty (the UI shows a message).
    try {
      await recorder.start();
    } catch (_) {
      return '';
    }

    final c = Completer<String>();
    _inflight = c;
    // Safety cap: auto-finish if the user never taps stop, so the mic never hangs open.
    _safety = Timer(timeout, () => unawaited(stop()));
    return c.future;
  }

  @override
  Future<void> stop() async {
    final c = _inflight;
    if (c == null || _finishing || _cancelled) return;
    _finishing = true;
    _safety?.cancel();

    RecordedAudio? clip;
    try {
      clip = await recorder.stop();
    } catch (_) {
      clip = null;
    }
    if (_cancelled) return _complete(c, '');
    if (clip == null || clip.bytes.isEmpty) return _complete(c, '');

    try {
      final transcript =
          (await transcribeCloud(audio: clip.bytes, language: language)).trim();
      _complete(c, transcript);
    } catch (e) {
      // Propagate the real failure (TranscribeException) so the field surfaces the reason rather
      // than the old generic message. Permission/empty/cancel still resolve '' (their own meaning).
      _completeError(c, e);
    }
  }

  @override
  Future<void> cancel() async {
    _cancelled = true;
    _safety?.cancel();
    final c = _inflight;
    try {
      await recorder.cancel();
    } catch (_) {/* best-effort */}
    if (c != null) _complete(c, '');
  }

  void _complete(Completer<String> c, String value) {
    if (!c.isCompleted) c.complete(value);
    _inflight = null;
    _finishing = false;
  }

  void _completeError(Completer<String> c, Object error) {
    if (!c.isCompleted) c.completeError(error);
    _inflight = null;
    _finishing = false;
  }
}
