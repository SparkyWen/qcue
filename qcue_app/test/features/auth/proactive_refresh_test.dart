// QCue AUTH-R5: the proactive refresh schedules a refresh at ~80% of the access
// TTL using the persisted expires_at, so the token is rotated BEFORE it expires
// and the background callers never see a synchronized 401 storm.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/features/auth/proactive_refresh.dart';

void main() {
  test('delayUntilRefresh returns ~80% of the remaining TTL', () {
    final now = DateTime.utc(2026, 6, 16, 10, 0, 0);
    final expiresAt = now.add(const Duration(seconds: 3600)); // 1h token
    final d = ProactiveRefresh.delayUntilRefresh(expiresAt, now: now);
    // 80% of 3600s = 2880s.
    expect(d, const Duration(seconds: 2880));
  });

  test('an already-expired token schedules an immediate refresh', () {
    final now = DateTime.utc(2026, 6, 16, 10, 0, 0);
    final past = now.subtract(const Duration(seconds: 10));
    final d = ProactiveRefresh.delayUntilRefresh(past, now: now);
    expect(d, Duration.zero);
  });

  test('a null expiry yields no schedule (caller falls back to refresh-on-401)', () {
    expect(ProactiveRefresh.delayUntilRefresh(null), isNull);
  });

  test('fires the refresh callback when the timer elapses', () async {
    var fired = 0;
    final now = DateTime.now().toUtc();
    final pr = ProactiveRefresh(
      refresh: () async {
        fired++;
        return true;
      },
    );
    // expiry just barely in the future so 80% delay is ~tens of ms.
    pr.schedule(now.add(const Duration(milliseconds: 50)), now: now);
    await Future<void>.delayed(const Duration(milliseconds: 120));
    expect(fired, 1);
    pr.cancel();
  });
}
