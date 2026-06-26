// QCue D4: the cloud-primary voice controller. capture() records via the AudioRecorderFacade, then on
// stop() uploads the bytes to the cloud STT and resolves with the transcript — NEVER auto-committing.
// Permission denial, an empty take, a cancel, and a transcription failure all resolve to '' so the
// capture flow stays alive (the field surfaces a message).
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/features/capture/widgets/native_voice_capture_controller.dart';

import '../../fakes/fake_audio_recorder.dart';

class _CloudSpy {
  _CloudSpy({this.reply = '', this.throws = false});
  List<int>? audio;
  String? language;
  String reply;
  bool throws;

  Future<String> transcribe({required List<int> audio, String? language}) async {
    this.audio = audio;
    this.language = language;
    if (throws) throw const TranscribeException('openai stt 400: model not found');
    return reply;
  }
}

void main() {
  test('records then transcribes via the cloud and returns the transcript',
      () async {
    final rec = FakeAudioRecorder(clip: Uint8List.fromList(const [9, 8, 7]));
    final cloud = _CloudSpy(reply: '你好世界');
    final c = NativeVoiceCaptureController(
      recorder: rec,
      transcribeCloud: cloud.transcribe,
      language: 'zh',
    );

    final fut = c.capture();
    await pumpEventQueue(); // let permission + start resolve; recording begins
    expect(rec.started, isTrue);
    await c.stop();
    expect(await fut, '你好世界');
    // the recorded bytes (not nothing) were uploaded, with the language hint.
    expect(cloud.audio, [9, 8, 7]);
    expect(cloud.language, 'zh');
  });

  test('denied permission resolves empty and never records', () async {
    final rec = FakeAudioRecorder(permission: false);
    final cloud = _CloudSpy(reply: 'should not happen');
    final c = NativeVoiceCaptureController(
        recorder: rec, transcribeCloud: cloud.transcribe);
    expect(await c.capture(), '');
    expect(rec.started, isFalse);
    expect(cloud.audio, isNull);
  });

  test('a transcription failure PROPAGATES so the field can show the real reason',
      () async {
    final rec = FakeAudioRecorder(clip: Uint8List.fromList(const [1]));
    final cloud = _CloudSpy(throws: true);
    final c = NativeVoiceCaptureController(
        recorder: rec, transcribeCloud: cloud.transcribe);
    final fut = c.capture();
    await pumpEventQueue();
    await c.stop();
    await expectLater(fut, throwsA(isA<TranscribeException>()));
  });

  test('cancel aborts with no transcript and never uploads', () async {
    final rec = FakeAudioRecorder(clip: Uint8List.fromList(const [1, 2]));
    final cloud = _CloudSpy(reply: 'nope');
    final c = NativeVoiceCaptureController(
        recorder: rec, transcribeCloud: cloud.transcribe);
    final fut = c.capture();
    await pumpEventQueue();
    await c.cancel();
    expect(await fut, '');
    expect(cloud.audio, isNull);
  });
}
