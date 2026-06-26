// QCue D4: a deterministic AudioRecorderFacade for host tests — no platform channel.
import 'dart:typed_data';

import 'package:qcue_app/core/native/audio/audio_recorder.dart';

class FakeAudioRecorder implements AudioRecorderFacade {
  FakeAudioRecorder({
    this.permission = true,
    Uint8List? clip,
  }) : clip = clip ?? Uint8List.fromList(const [1, 2, 3, 4]);

  bool permission;
  Uint8List clip;

  bool started = false;
  bool cancelled = false;

  @override
  Future<bool> ensurePermission() async => permission;

  @override
  Future<void> start() async {
    started = true;
    cancelled = false;
  }

  @override
  Future<RecordedAudio?> stop() async {
    if (!started || cancelled) return null;
    started = false;
    if (clip.isEmpty) return null;
    return RecordedAudio(bytes: clip);
  }

  @override
  Future<void> cancel() async {
    cancelled = true;
    started = false;
  }
}
