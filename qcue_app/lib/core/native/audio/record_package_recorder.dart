// QCue D4: the device AudioRecorderFacade backed by the `record` plugin (AVAudioRecorder on iOS,
// MediaRecorder on Android). Records m4a/AAC mono to a temp file, then reads the bytes back for upload.
// Only constructed at bootstrap on a real device; host tests use FakeAudioRecorder, so this file's
// plugin/dart:io use never runs under `flutter test`.
import 'dart:async';
import 'dart:io';
import 'dart:typed_data';

import 'package:path_provider/path_provider.dart';
import 'package:record/record.dart';

import 'audio_recorder.dart';

class RecordPackageRecorder implements AudioRecorderFacade {
  RecordPackageRecorder();

  final AudioRecorder _recorder = AudioRecorder();
  String? _path;

  /// A real take is ≥ a few KB; a header-only m4a (just the ~28-byte `ftyp` box, no audio frames) is a
  /// FAILED capture. We discard anything below this so we surface "no speech / check mic" instead of
  /// uploading an unreadable clip the speech service rejects as "corrupted". OpenAI also rejects clips
  /// this short, so the floor costs no real recordings.
  static const int _minClipBytes = 2048;

  @override
  Future<bool> ensurePermission() => _recorder.hasPermission();

  @override
  Future<void> start() async {
    final dir = await getTemporaryDirectory();
    // A unique name per take so a slow upload can't be clobbered by the next record.
    final path =
        '${dir.path}/qcue_voice_${DateTime.now().microsecondsSinceEpoch}.m4a';
    _path = path;
    await _recorder.start(
      const RecordConfig(
        encoder: AudioEncoder.aacLc,
        // 44.1 kHz is the hardware-native capture rate on iOS (the record plugin's default). Forcing
        // 16 kHz made iOS AVAudioRecorder capture NOTHING on-device — only the 28-byte `ftyp` header
        // reached the server (→ OpenAI "Audio file might be corrupted"). The cloud STT resamples, so a
        // higher rate is correct and still a small upload. Do NOT lower this without on-device testing.
        sampleRate: 44100,
        numChannels: 1,
      ),
      path: path,
    );
  }

  @override
  Future<RecordedAudio?> stop() async {
    final path = await _recorder.stop() ?? _path;
    _path = null;
    if (path == null) return null;
    final file = File(path);
    // iOS AVAudioRecorder can finalize the file (mdat/moov) just after stop() returns; wait for the
    // size to settle so we never read a half-written clip.
    final bytes = await _readFinalized(file);
    unawaited(_deleteQuietly(file)); // best-effort cleanup
    // A null/empty read or a header-only file means no audio was captured → treat as an empty take.
    if (bytes == null || bytes.length < _minClipBytes) return null;
    return RecordedAudio(bytes: bytes);
  }

  /// Read the clip once its on-disk size has stopped growing (≈ finalized), capped so a stuck file
  /// can't hang the UI. Returns null if the file never appears or is empty.
  Future<Uint8List?> _readFinalized(File file) async {
    const tick = Duration(milliseconds: 80);
    var last = -1;
    for (var i = 0; i < 12; i++) {
      // ≤ ~1s total
      if (!await file.exists()) {
        await Future<void>.delayed(tick);
        continue;
      }
      final len = await file.length();
      if (len > 0 && len == last) break; // size stable across two ticks → finalized
      last = len;
      await Future<void>.delayed(tick);
    }
    if (!await file.exists()) return null;
    final bytes = await file.readAsBytes();
    return bytes.isEmpty ? null : bytes;
  }

  @override
  Future<void> cancel() async {
    final path = _path;
    _path = null;
    try {
      await _recorder.cancel();
    } finally {
      if (path != null) unawaited(_deleteQuietly(File(path)));
    }
  }

  Future<void> _deleteQuietly(File f) async {
    try {
      if (await f.exists()) await f.delete();
    } catch (_) {/* the OS clears the temp dir regardless */}
  }
}
