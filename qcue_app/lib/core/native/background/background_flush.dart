// QCue S5-R37/R38/R41: the background-flush facade.
//
// Control flows over MethodChannel('qcue/background'). The facade:
//   - schedulePeriodic(): ask the OS to register a periodic, network-gated
//     background task that wakes the app to drain the offline outbound capture
//     queue (S5-R37). The schedule is idempotent — re-scheduling REPLACES the
//     single unique work item, it does not stack duplicates.
//   - runFlush(): invoked by the OS task callback / on a reachability transition;
//     it delegates to the injected idempotent flush seam (the offline queue
//     dedupes on client id, S5-R38) and SWALLOWS errors so a slow/absent network
//     never crashes the background task (S5-R41).
//
//   - Native Android: a WorkManager periodic request with a network constraint,
//     enqueued with KEEP/REPLACE so it stays unique.
//   - Native iOS: a BGAppRefreshTask registered with BGTaskScheduler; the task
//     handler invokes the Dart `runFlush` over the channel.
import 'dart:async';

import 'package:flutter/services.dart';
import '../channels.dart';

/// The idempotent drain seam. The bootstrap binds this to
/// `OfflineAwareApiClient.flushOutbox` (POST each queued capture once, dedupe on
/// retry), so a repeated flush never double-POSTs (S5-R38).
typedef FlushQueue = Future<void> Function();

class BackgroundFlush {
  BackgroundFlush({
    required FlushQueue flush,
    this._method = const MethodChannel(QcueChannels.background),
  }) :
        // ignore: prefer_initializing_formals — keep the readable `flush:` name
        _flush = flush {
    // Receive the OS task callback: iOS's BGTask handler invokes `runFlush` over
    // this channel — previously it landed on a dead channel and nothing drained.
    // (Android drains via a headless engine on its own channel; see FlushWorker.)
    _method.setMethodCallHandler(_onNativeCall);
  }

  final FlushQueue _flush;
  final MethodChannel _method;

  Future<dynamic> _onNativeCall(MethodCall call) async {
    if (call.method == 'runFlush') await runFlush();
    return null;
  }

  /// Stop receiving native callbacks (e.g. on host teardown).
  void dispose() => _method.setMethodCallHandler(null);

  /// Register the OS background task (S5-R37). `requiresNetwork:true` means the
  /// OS only wakes us when online; `replace:true` keeps a single unique work
  /// item so re-scheduling on every launch does not stack duplicates.
  Future<void> schedulePeriodic() async {
    await _method.invokeMethod<void>(
      'schedulePeriodic',
      QcueChannels.envelope({'requiresNetwork': true, 'replace': true}),
    );
  }

  /// Run the flush now (the OS task callback / a reachability transition). Errors
  /// are swallowed so a failed network never crashes the background task — the
  /// queue stays put and the next window retries (S5-R38/R41).
  Future<void> runFlush() async {
    try {
      await _flush();
    } catch (_) {
      // offline / server unreachable: leave the queue for the next window.
    }
  }

  /// Cancel the scheduled OS task.
  Future<void> cancel() async {
    await _method.invokeMethod<void>('cancel', QcueChannels.envelope());
  }
}
