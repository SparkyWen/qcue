// QCue AUTH-R5: proactive token refresh. Schedules a single-flight refresh at
// ~80% of the access-token TTL using the persisted expires_at, so the token is
// rotated BEFORE the background callers (connectivity probe @10s, sync pull @30s,
// feed reads, offline flush) hit a synchronized 401 storm. A null/garbage expiry
// yields no schedule — the refresh-on-401 path remains the safety net.
import 'dart:async';

class ProactiveRefresh {
  ProactiveRefresh({required Future<bool> Function() refresh})
      // ignore: prefer_initializing_formals
      : _refresh = refresh;

  final Future<bool> Function() _refresh;
  Timer? _timer;

  /// Fraction of the access TTL at which to refresh (refresh at 80% elapsed).
  static const _refreshFraction = 0.8;

  /// The delay from [now] until the proactive refresh should fire: 80% of the
  /// remaining lifetime. `null` when there is no expiry (fall back to 401-driven
  /// refresh); [Duration.zero] when the token is already at/past the threshold.
  static Duration? delayUntilRefresh(DateTime? expiresAt, {DateTime? now}) {
    if (expiresAt == null) return null;
    final n = (now ?? DateTime.now()).toUtc();
    final remaining = expiresAt.toUtc().difference(n);
    if (remaining <= Duration.zero) return Duration.zero;
    final ms = (remaining.inMilliseconds * _refreshFraction).round();
    return Duration(milliseconds: ms < 0 ? 0 : ms);
  }

  /// (Re)arm the timer from [expiresAt]. A no-op when there is no expiry. Cancels
  /// any prior timer so a fresh login/refresh re-bases the schedule.
  void schedule(DateTime? expiresAt, {DateTime? now}) {
    final delay = delayUntilRefresh(expiresAt, now: now);
    _timer?.cancel();
    if (delay == null) return;
    _timer = Timer(delay, () {
      // best-effort: the single-flight refresh coalesces with any 401-driven one.
      unawaited(_refresh());
    });
  }

  void cancel() {
    _timer?.cancel();
    _timer = null;
  }
}
