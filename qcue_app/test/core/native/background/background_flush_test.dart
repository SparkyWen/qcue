// QCue S5-R37/R38/R41: the background-flush facade over MethodChannel('qcue/background').
// It SCHEDULES an OS background task (WorkManager / BGTaskScheduler) that drains
// the offline outbound capture queue, and exposes runFlush() — invoked by the OS
// task callback / on reachability — which delegates to the injected, idempotent
// flush seam. A repeated flush never double-POSTs (the queue dedupes on client
// id, S5-R38); the schedule call is idempotent too.
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/native/background/background_flush.dart';
import 'package:qcue_app/core/native/channels.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  const method = MethodChannel(QcueChannels.background);

  late List<MethodCall> calls;
  late int flushCount;

  setUp(() {
    calls = [];
    flushCount = 0;
    messenger.setMockMethodCallHandler(method, (call) async {
      calls.add(call);
      final args = (call.arguments as Map?)?.cast<String, dynamic>() ?? {};
      expect(args['schemaVersion'], QcueChannels.schemaVersion);
      return null;
    });
  });

  tearDown(() => messenger.setMockMethodCallHandler(method, null));

  BackgroundFlush build() => BackgroundFlush(flush: () async => flushCount++);

  test('S5-R37: schedule asks the OS to register a periodic network-gated task',
      () async {
    final bg = build();
    await bg.schedulePeriodic();
    final c = calls.firstWhere((c) => c.method == 'schedulePeriodic');
    final args = (c.arguments as Map).cast<String, dynamic>();
    // requires-network constraint is carried so the OS only wakes us online.
    expect(args['requiresNetwork'], isTrue);
    expect(args['schemaVersion'], QcueChannels.schemaVersion);
  });

  test('S5-R37: schedule is idempotent (re-scheduling replaces, not stacks)',
      () async {
    final bg = build();
    await bg.schedulePeriodic();
    await bg.schedulePeriodic();
    // both calls carry replace=true so the OS keeps a single unique work item.
    for (final c in calls.where((c) => c.method == 'schedulePeriodic')) {
      expect((c.arguments as Map)['replace'], isTrue);
    }
  });

  test('S5-R38: runFlush delegates to the injected flush seam', () async {
    final bg = build();
    await bg.runFlush();
    expect(flushCount, 1);
  });

  test('S5-R38: a repeated flush is safe (idempotent — never throws/double)',
      () async {
    final bg = build();
    await bg.runFlush();
    await bg.runFlush();
    await bg.runFlush();
    expect(flushCount, 3); // each call runs the (idempotent) drain once
  });

  test('S5-R41: a throwing flush is swallowed (background task never crashes)',
      () async {
    final bg = BackgroundFlush(flush: () async => throw StateError('offline'));
    // must complete normally — a slow/failed network never crashes the OS task.
    await bg.runFlush();
  });

  test('cancel forwards to the OS scheduler', () async {
    final bg = build();
    await bg.cancel();
    expect(calls.any((c) => c.method == 'cancel'), isTrue);
  });

  test('S5-R37: an inbound runFlush from the OS task drains the queue', () async {
    // The keystone the background flush was missing: a Dart receiver for the
    // native→Dart runFlush call (iOS BGTask handler invokes it; the Android
    // headless engine invokes it on its own channel). Without this it landed on
    // a dead channel and nothing drained.
    final bg = build();
    await messenger.handlePlatformMessage(
      QcueChannels.background,
      const StandardMethodCodec().encodeMethodCall(const MethodCall('runFlush')),
      (_) {},
    );
    expect(flushCount, 1);
    bg.dispose();
  });
}
