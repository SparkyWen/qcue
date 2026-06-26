// QCue S5-R30/R33/R34/R36: the NotifChannel facade over MethodChannel('qcue/notif')
// + EventChannel('qcue/notif/events'). The facade:
//   - requests notification permission (just-in-time, S5-R30);
//   - shows a LocalNotif for each of the three QNotifKinds with its honest title
//     + deep-link route (S5-R33/R36);
//   - on a notification TAP, deep-links to the right go_router route (S5-R34),
//     idempotently; an unknown kind is dropped (S5-R33).
// All against the SDK mock messenger (no device).
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/native/channels.dart';
import 'package:qcue_app/core/native/notif/notif_channel.dart';
import 'package:qcue_app/core/native/protocol/native_dtos.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  const method = MethodChannel(QcueChannels.notif);
  const event = MethodChannel(QcueChannels.notifEvents);

  late List<MethodCall> calls;
  late List<String> deepLinks;
  String permission = 'granted';

  void emit(Object? e) {
    messenger.handlePlatformMessage(
      QcueChannels.notifEvents,
      const StandardMethodCodec().encodeSuccessEnvelope(e),
      (_) {},
    );
  }

  setUp(() {
    calls = [];
    deepLinks = [];
    permission = 'granted';
    messenger.setMockMethodCallHandler(method, (call) async {
      calls.add(call);
      final args = (call.arguments as Map?)?.cast<String, dynamic>() ?? {};
      expect(args['schemaVersion'], QcueChannels.schemaVersion);
      switch (call.method) {
        case 'requestPermission':
          return permission;
        case 'show':
          return null;
        case 'cancelKind':
          return null;
        case 'registerPushToken':
          return null;
        default:
          return null;
      }
    });
    messenger.setMockMethodCallHandler(event, (call) async => null);
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(method, null);
    messenger.setMockMethodCallHandler(event, null);
  });

  NotifChannel build() => NotifChannel(onDeepLink: (r) => deepLinks.add(r));

  test('S5-R30: requestPermission returns the parsed status', () async {
    final n = build();
    expect(await n.requestPermission(), NotifPermission.granted);
    permission = 'denied';
    expect(await n.requestPermission(), NotifPermission.denied);
  });

  test('S5-R36: showing dreamComplete sends the server count + route', () async {
    final n = build();
    await n.show(LocalNotif.dreamComplete(pages: 4, jobId: 'job-2'));
    final show = calls.firstWhere((c) => c.method == 'show');
    final args = (show.arguments as Map).cast<String, dynamic>();
    expect(args['kind'], 'dreamComplete');
    expect(args['title'], 'QCue improved 4 pages');
    expect(args['route'], {'id': 'job-2'});
    expect(args['schemaVersion'], QcueChannels.schemaVersion);
  });

  test('S5-R33: each of the three kinds maps to its deep-link on tap', () async {
    final n = build();
    n.start();
    await pumpEventQueue();

    emit({'kind': 'dreamComplete', 'route': {'id': 'job-9'}});
    emit({'kind': 'ingestNeedsReview', 'route': const {}});
    emit({'kind': 'syncConflict', 'route': const {}});
    await pumpEventQueue();

    expect(deepLinks, [
      '/settings/activity/dream/job-9',
      '/settings/activity',
      '/settings/activity',
    ]);
    await n.dispose();
  });

  test('S5-R33: an unknown notif kind on the tap stream is dropped', () async {
    final n = build();
    n.start();
    await pumpEventQueue();
    emit({'kind': 'brandNew', 'route': const {}});
    await pumpEventQueue();
    expect(deepLinks, isEmpty);
    await n.dispose();
  });

  test('S5-R34: tapping the same dream notification twice routes the same place',
      () async {
    final n = build();
    n.start();
    await pumpEventQueue();
    emit({'kind': 'dreamComplete', 'route': {'id': 'job-1'}});
    emit({'kind': 'dreamComplete', 'route': {'id': 'job-1'}});
    await pumpEventQueue();
    // go_router de-dupes navigation to the same location; both resolve identically.
    expect(deepLinks,
        ['/settings/activity/dream/job-1', '/settings/activity/dream/job-1']);
    expect(deepLinks.toSet(), {'/settings/activity/dream/job-1'});
    await n.dispose();
  });

  test('cancelKind forwards the closed kind token', () async {
    final n = build();
    await n.cancelKind(QNotifKind.ingestNeedsReview);
    final c = calls.firstWhere((c) => c.method == 'cancelKind');
    expect((c.arguments as Map)['kind'], 'ingestNeedsReview');
  });

  test('S3-roadmap: registerPushToken is a documented stub that no-ops cleanly',
      () async {
    final n = build();
    await n.registerPushToken(); // push/FCM/APNs is roadmap — does not throw
    expect(calls.any((c) => c.method == 'registerPushToken'), isTrue);
  });
}
