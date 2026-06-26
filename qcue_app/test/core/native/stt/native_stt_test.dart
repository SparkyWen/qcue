// QCue S5-R18/R19/R21: the NativeStt facade over MethodChannel('qcue/stt') +
// EventChannel('qcue/stt/events'). The method channel carries isAvailable /
// requestPermission / start(locale) / stop with the right args + schemaVersion;
// the event channel surfaces partial -> final transcripts + errors + an
// availability event. All against the SDK mock messenger (no device).
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/native/channels.dart';
import 'package:qcue_app/core/native/stt/native_stt.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  const method = MethodChannel(QcueChannels.stt);
  const event = MethodChannel(QcueChannels.sttEvents);

  late List<MethodCall> calls;
  bool available = true;
  String permission = 'granted';

  // Drives the EventChannel: the mock handler returns null for listen/cancel,
  // and `emit` pushes an event back over the same channel name as the platform
  // side would, decoded by Dart's StandardMethodCodec.
  void emit(Object? event) {
    messenger.handlePlatformMessage(
      QcueChannels.sttEvents,
      const StandardMethodCodec().encodeSuccessEnvelope(event),
      (_) {},
    );
  }

  void emitError(String code, String message) {
    messenger.handlePlatformMessage(
      QcueChannels.sttEvents,
      const StandardMethodCodec().encodeErrorEnvelope(code: code, message: message),
      (_) {},
    );
  }

  setUp(() {
    calls = [];
    available = true;
    permission = 'granted';
    messenger.setMockMethodCallHandler(method, (call) async {
      calls.add(call);
      final args = (call.arguments as Map?)?.cast<String, dynamic>() ?? {};
      expect(args['schemaVersion'], QcueChannels.schemaVersion);
      switch (call.method) {
        case 'isAvailable':
          return available;
        case 'requestPermission':
          return permission;
        case 'start':
          return null;
        case 'stop':
          return null;
        default:
          throw PlatformException(code: 'osError', message: 'unknown');
      }
    });
    // The EventChannel uses the SAME messenger; a mock handler that accepts
    // listen/cancel lets the broadcast stream subscribe in the test harness.
    messenger.setMockMethodCallHandler(event, (call) async => null);
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(method, null);
    messenger.setMockMethodCallHandler(event, null);
  });

  test('S5-R18: isAvailable forwards the locale + schema version', () async {
    const stt = NativeStt();
    expect(await stt.isAvailable(locale: 'en-US'), isTrue);
    available = false;
    expect(await stt.isAvailable(locale: 'xx-ZZ'), isFalse);
    final call = calls.firstWhere((c) => c.method == 'isAvailable');
    final args = (call.arguments as Map).cast<String, dynamic>();
    expect(args['localeTag'], 'en-US');
    expect(args['schemaVersion'], QcueChannels.schemaVersion);
  });

  test('S5-R18: requestPermission returns the parsed status', () async {
    const stt = NativeStt();
    expect(await stt.requestPermission(), SttPermission.granted);
    permission = 'denied';
    expect(await stt.requestPermission(), SttPermission.denied);
  });

  test('S5-R18: start forwards locale, partials flag + a capture id', () async {
    const stt = NativeStt();
    await stt.start(locale: 'zh-CN', captureId: 'cap-1');
    final call = calls.firstWhere((c) => c.method == 'start');
    final args = (call.arguments as Map).cast<String, dynamic>();
    expect(args['localeTag'], 'zh-CN');
    expect(args['captureId'], 'cap-1');
    expect(args['partialResults'], isTrue);
    expect(args['schemaVersion'], QcueChannels.schemaVersion);
  });

  test('S5-R19: the event stream surfaces partial -> final transcripts',
      () async {
    const stt = NativeStt();
    final events = <SttEvent>[];
    final sub = stt.events.listen(events.add);
    await pumpEventQueue();

    emit({'event': 'partial', 'captureId': 'c1', 'text': 'hel'});
    emit({'event': 'partial', 'captureId': 'c1', 'text': 'hello'});
    emit({
      'event': 'final',
      'captureId': 'c1',
      'transcript': 'hello world',
      'onDevice': true,
      'confidence': 0.92,
      'localeTag': 'en-US',
      'audioMillis': 1500,
    });
    await pumpEventQueue();

    expect(events.whereType<SttPartial>().map((e) => e.text),
        ['hel', 'hello']);
    final fin = events.whereType<SttFinal>().single;
    expect(fin.transcript, 'hello world');
    expect(fin.onDevice, isTrue);
    expect(fin.confidence, closeTo(0.92, 1e-9));
    expect(fin.audioMillis, 1500);
    await sub.cancel();
  });

  test('S5-R21: an error event surfaces a typed SttError', () async {
    const stt = NativeStt();
    final events = <SttEvent>[];
    final sub = stt.events.listen(events.add);
    await pumpEventQueue();

    emit({
      'event': 'error',
      'captureId': 'c1',
      'kind': 'unsupportedLocale',
      'message': 'no recognizer for xx-ZZ',
    });
    await pumpEventQueue();

    final err = events.whereType<SttError>().single;
    expect(err.kind, SttErrorKind.unsupportedLocale);
    expect(err.message, contains('xx-ZZ'));
    await sub.cancel();
  });

  test('availability event reports on-device + supported locales', () async {
    const stt = NativeStt();
    final events = <SttEvent>[];
    final sub = stt.events.listen(events.add);
    await pumpEventQueue();
    emit({
      'event': 'avail',
      'onDeviceAvailable': true,
      'supportedLocales': ['en-US', 'zh-CN'],
    });
    await pumpEventQueue();
    final avail = events.whereType<SttAvail>().single;
    expect(avail.onDeviceAvailable, isTrue);
    expect(avail.supportedLocales, contains('zh-CN'));
    await sub.cancel();
  });

  test('a platform error envelope surfaces as an SttError(osError)', () async {
    const stt = NativeStt();
    final events = <SttEvent>[];
    final sub = stt.events.listen(events.add);
    await pumpEventQueue();
    emitError('osError', 'recognizer crashed');
    await pumpEventQueue();
    expect(events.whereType<SttError>().single.kind, SttErrorKind.osError);
    await sub.cancel();
  });
}
