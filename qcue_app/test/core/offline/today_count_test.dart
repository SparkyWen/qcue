// The home-widget "today" count must bucket by the USER's local calendar day, not a rolling 24h
// window. Exercised under a non-UTC zone (TZ=Australia/Sydney) where UTC-day != local-day.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/offline/today_count.dart';

Idea _at(DateTime capturedUtc) => Idea(
      id: 'x',
      tenantId: 't',
      userId: 'u',
      kind: IdeaKind.text,
      body: 'b',
      origin: 'capture',
      ingestState: IngestState.ingested,
      capturedAt: capturedUtc,
    );

void main() {
  test('counts by local calendar day, not a rolling 24h window', () {
    // localNow = 2026-06-16 10:00 Sydney (UTC+10) == 2026-06-16 00:00 UTC.
    final localNow = DateTime.utc(2026, 6, 16, 0, 0).toLocal();

    // (a) captured 2026-06-16 07:00 LOCAL (== 2026-06-15 21:00 UTC) → TODAY locally.
    final todayLocal = _at(DateTime.utc(2026, 6, 15, 21, 0));
    // (b) captured 2026-06-15 23:00 LOCAL (== 2026-06-15 13:00 UTC) → YESTERDAY locally, but it is
    //     within the last 24h of localNow — a rolling-24h count would WRONGLY include it.
    final yesterdayWithin24h = _at(DateTime.utc(2026, 6, 15, 13, 0));

    final n = todaysLocalCount([todayLocal, yesterdayWithin24h], localNow);
    expect(n, 1, reason: 'only the capture on the local "today" counts');
  }, skip: DateTime.now().timeZoneOffset == Duration.zero);
}
