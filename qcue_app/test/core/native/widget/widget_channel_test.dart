// QCue S5-R45/R46/R47: the WidgetChannel facade over MethodChannel('qcue/widget')
// + EventChannel('qcue/widget/events'). The facade:
//   (a) refreshes the widget's displayed state — a NON-SENSITIVE capture count
//       only (S5-R46) — and asks the OS to reload its timeline (S5-R47);
//   (b) handles a widget tap: a `compose` action DEEP-LINKS into the Capture
//       screen; a `quickCapture` action BACKGROUND-ENQUEUES a capture
//       (offline-safe, origin='capture') without a full app launch (S5-R45).
// All against the SDK mock messenger (no device).
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/native/channels.dart';
import 'package:qcue_app/core/native/protocol/native_dtos.dart';
import 'package:qcue_app/core/native/widget/widget_channel.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  const method = MethodChannel(QcueChannels.widget);
  const event = MethodChannel(QcueChannels.widgetEvents);

  late List<MethodCall> calls;
  late List<CaptureEnqueueReq> enqueued;
  late List<String> deepLinks;

  void emit(Object? e) {
    messenger.handlePlatformMessage(
      QcueChannels.widgetEvents,
      const StandardMethodCodec().encodeSuccessEnvelope(e),
      (_) {},
    );
  }

  setUp(() {
    calls = [];
    enqueued = [];
    deepLinks = [];
    messenger.setMockMethodCallHandler(method, (call) async {
      calls.add(call);
      final args = (call.arguments as Map?)?.cast<String, dynamic>() ?? {};
      expect(args['schemaVersion'], QcueChannels.schemaVersion);
      return null;
    });
    messenger.setMockMethodCallHandler(event, (call) async => null);
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(method, null);
    messenger.setMockMethodCallHandler(event, null);
  });

  WidgetChannel build() => WidgetChannel(
        enqueue: (req) async => enqueued.add(req),
        onDeepLink: (route) => deepLinks.add(route),
      );

  test('S5-R47: refresh writes the count then reloads the OS timeline', () async {
    final w = build();
    await w.refresh(todayCount: 3);
    final setCall = calls.firstWhere((c) => c.method == 'setCount');
    expect((setCall.arguments as Map)['count'], 3);
    expect(calls.any((c) => c.method == 'reloadTimelines'), isTrue);
  });

  test('S5-R46: refresh sends ONLY a count, never idea/body content', () async {
    final w = build();
    await w.refresh(todayCount: 7);
    for (final c in calls) {
      final args = (c.arguments as Map).cast<String, dynamic>();
      // the only non-envelope payload key is `count` — no body/title/text leaks.
      final keys = args.keys.where((k) => k != 'schemaVersion').toSet();
      expect(keys.difference({'count'}), isEmpty);
    }
  });

  test('S5-R45: a compose tap deep-links to the capture field', () async {
    final w = build();
    w.start();
    await pumpEventQueue();
    emit({'action': 'compose'});
    await pumpEventQueue();
    expect(deepLinks, ['/capture/compose']);
    expect(enqueued, isEmpty);
    await w.dispose();
  });

  test('S5-R45: a quickCapture tap background-enqueues (origin=capture)',
      () async {
    final w = build();
    w.start();
    await pumpEventQueue();
    emit({'action': 'quickCapture', 'args': {'body': 'quick note'}});
    await pumpEventQueue();
    expect(enqueued, hasLength(1));
    expect(enqueued.single.origin, 'capture:widget');
    expect(enqueued.single.body, 'quick note');
    expect(deepLinks, isEmpty); // no app-launch deep link for a quick capture
    await w.dispose();
  });

  test('S5-R45: a quickCapture with no body enqueues an empty voice-stub',
      () async {
    final w = build();
    w.start();
    await pumpEventQueue();
    emit({'action': 'quickCapture'});
    await pumpEventQueue();
    expect(enqueued.single.origin, 'capture:widget');
    expect(enqueued.single.body, '');
    await w.dispose();
  });

  test('an unknown widget action is ignored (no enqueue, no deep-link)',
      () async {
    final w = build();
    w.start();
    await pumpEventQueue();
    emit({'action': 'somethingElse'});
    await pumpEventQueue();
    expect(enqueued, isEmpty);
    expect(deepLinks, isEmpty);
    await w.dispose();
  });
}
