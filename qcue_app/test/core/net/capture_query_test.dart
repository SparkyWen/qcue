// QCue: a date-scoped day window must span exactly the LOCAL calendar day — even on DST transition days
// (23h/25h) — and agree with sameLocalDay, so the online (Http) day-view matches the Stub/offline view.
// Run under a DST timezone (e.g. TZ=America/New_York) to exercise the transition days deterministically.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/capture_query.dart';

void main() {
  test('utcDayBounds spans exactly the local calendar day (DST-safe) + rolls month/year', () {
    final days = [
      DateTime(2026, 3, 8), // US spring-forward (23h day)
      DateTime(2026, 11, 1), // US fall-back (25h day)
      DateTime(2026, 1, 31), // month rollover
      DateTime(2026, 12, 31), // year rollover
      DateTime(2026, 6, 15), // ordinary day
    ];
    for (final day in days) {
      final (start, end) = utcDayBounds(day);
      // Back in local time the bounds must be midnight → next local midnight, regardless of DST length.
      expect(start.toLocal(), DateTime(day.year, day.month, day.day), reason: 'start = local midnight of $day');
      expect(end.toLocal(), DateTime(day.year, day.month, day.day + 1), reason: 'end = next local midnight of $day');
    }
  });

  test('utcDayBounds agrees with sameLocalDay on the boundary', () {
    final day = DateTime(2026, 11, 1); // a DST fall-back day
    final (start, end) = utcDayBounds(day);
    final lastInstant = end.subtract(const Duration(seconds: 1)); // 23:59:59 local of `day`
    expect(sameLocalDay(start, day), isTrue, reason: 'first instant belongs to the day');
    expect(sameLocalDay(lastInstant, day), isTrue, reason: "the day's last second belongs to the day");
    expect(sameLocalDay(end, day), isFalse, reason: 'the exclusive upper bound is the next day');
  });
}
