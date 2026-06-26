// QCue D4: the audio-recorder seam (fake). The real plugin impl is device-bound.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/native/audio/audio_recorder.dart';

import '../fakes/fake_audio_recorder.dart';

void main() {
  test('records and returns a clip; cancel discards it', () async {
    final rec = FakeAudioRecorder();
    expect(await rec.ensurePermission(), isTrue);

    await rec.start();
    final RecordedAudio? clip = await rec.stop();
    expect(clip, isNotNull);
    expect(clip!.bytes, isNotEmpty);
    expect(clip.mime, 'audio/m4a');

    // a cancelled take yields nothing.
    await rec.start();
    await rec.cancel();
    expect(await rec.stop(), isNull);
  });

  test('denied permission is surfaced', () async {
    final rec = FakeAudioRecorder(permission: false);
    expect(await rec.ensurePermission(), isFalse);
  });
}
