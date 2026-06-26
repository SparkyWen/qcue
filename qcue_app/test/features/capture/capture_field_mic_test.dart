// QCue v0.2.1 mic-hang fix: the Capture field's mic is tap-to-start /
// tap-to-stop. The button MUST stay enabled while listening (so the user can
// stop), show a stop affordance, commit the transcript as a 'voice' origin on
// stop, and cancel the in-flight capture on dispose so a left-open mic never
// hangs. Driven by a fake VoiceCaptureController — no platform channel.
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/capture/widgets/capture_field.dart';
import 'package:qcue_app/features/capture/widgets/voice_capture_controller.dart';

class FakeVoice implements VoiceCaptureController {
  final Completer<String> _c = Completer<String>();
  bool captureStarted = false;
  int stopCalls = 0;
  int cancelCalls = 0;

  @override
  Future<String> capture() {
    captureStarted = true;
    return _c.future;
  }

  @override
  Future<void> stop() async => stopCalls++;

  @override
  Future<void> cancel() async {
    cancelCalls++;
    if (!_c.isCompleted) _c.complete('');
  }

  /// Simulate the recognizer producing its final after the user stops.
  void finish(String t) {
    if (!_c.isCompleted) _c.complete(t);
  }

  /// Simulate a transcription failure surfacing the server's real reason.
  void fail(Object error) {
    if (!_c.isCompleted) _c.completeError(error);
  }
}

Widget _wrap(Widget child) => MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(body: child),
    );

const _mic = ValueKey('mic-button');

void main() {
  testWidgets('tapping starts recording — button stays enabled, shows stop',
      (tester) async {
    final voice = FakeVoice();
    await tester.pumpWidget(_wrap(CaptureField(onCommit: (_, __) {}, voice: voice)));

    await tester.tap(find.byKey(_mic));
    await tester.pump();

    expect(voice.captureStarted, isTrue);
    expect(find.byIcon(Icons.stop_rounded), findsOneWidget,
        reason: 'a stop affordance is shown while listening');
    final btn = tester.widget<IconButton>(find.byKey(_mic));
    expect(btn.onPressed, isNotNull,
        reason: 'must stay tappable so the user can stop (the hang root cause)');
  });

  testWidgets('D4: tapping again stops and loads the transcript into the field for review',
      (tester) async {
    final committed = <String>[];
    final voice = FakeVoice();
    await tester.pumpWidget(_wrap(
        CaptureField(onCommit: (b, o) => committed.add('$o:$b'), voice: voice)));

    await tester.tap(find.byKey(_mic)); // start
    await tester.pump();
    await tester.tap(find.byKey(_mic)); // stop
    await tester.pump();
    expect(voice.stopCalls, 1);

    voice.finish('hello from voice'); // cloud STT returns the transcript
    await tester.pump();

    // The transcript is loaded into the editable field — NOT auto-committed.
    expect(committed, isEmpty, reason: 'voice no longer auto-publishes');
    final field = tester.widget<TextField>(find.byKey(const ValueKey('capture-field')));
    expect(field.controller!.text, 'hello from voice');
    expect(find.byIcon(Icons.mic_none_outlined), findsOneWidget,
        reason: 'returns to idle after the take resolves');
  });

  testWidgets('an empty take surfaces a message instead of silently flipping back',
      (tester) async {
    final committed = <String>[];
    final voice = FakeVoice();
    await tester.pumpWidget(_wrap(
        CaptureField(onCommit: (b, o) => committed.add('$o:$b'), voice: voice)));

    await tester.tap(find.byKey(_mic)); // start
    await tester.pump();
    voice.finish(''); // produced nothing (no speech / offline / no key / denied)
    await tester.pump(); // capture() resolves
    await tester.pump(); // SnackBar animates in

    expect(committed, isEmpty, reason: 'nothing to commit');
    expect(find.byKey(const ValueKey('mic-no-transcript')), findsOneWidget,
        reason: 'the user is told the take produced nothing, not left guessing');
    expect(find.byIcon(Icons.mic_none_outlined), findsOneWidget,
        reason: 'returns to idle');
  });

  testWidgets("a transcription failure shows the server's real reason, not a generic line",
      (tester) async {
    final voice = FakeVoice();
    await tester.pumpWidget(_wrap(
        CaptureField(onCommit: (_, __) {}, voice: voice)));

    await tester.tap(find.byKey(_mic)); // start
    await tester.pump();
    voice.fail(const TranscribeException('openai stt 400: model not found'));
    await tester.pump(); // capture() completes with the error
    await tester.pump(); // SnackBar animates in

    expect(find.byKey(const ValueKey('mic-no-transcript')), findsOneWidget);
    expect(find.textContaining('model not found'), findsOneWidget,
        reason: 'the real provider error is surfaced');
    expect(find.byIcon(Icons.mic_none_outlined), findsOneWidget,
        reason: 'returns to idle after a failure');
  });

  testWidgets('disposing while listening cancels the in-flight capture',
      (tester) async {
    final voice = FakeVoice();
    await tester.pumpWidget(_wrap(CaptureField(onCommit: (_, __) {}, voice: voice)));

    await tester.tap(find.byKey(_mic));
    await tester.pump();

    await tester.pumpWidget(_wrap(const SizedBox.shrink())); // dispose the field
    expect(voice.cancelCalls, 1);
  });
}
