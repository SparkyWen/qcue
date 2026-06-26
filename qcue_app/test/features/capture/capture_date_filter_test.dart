// QCue: captures(day:) scopes the feed to a chosen LOCAL calendar day so the Capture screen's calendar
// picker can show ALL of that day's captures. Exercises the production StubApiClient filter.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';

void main() {
  test('captures(day:) returns only that local day; unscoped feed is unchanged', () async {
    final api = StubApiClient.seeded();
    await api.capture(body: 'today thought', origin: 'capture'); // capturedAt = now (today)

    final today = await api.captures(day: DateTime.now());
    expect(today.any((i) => i.body == 'today thought'), isTrue,
        reason: "today's day-view must include a capture made today");
    for (final i in today) {
      final l = i.capturedAt.toLocal();
      final now = DateTime.now();
      expect(l.year == now.year && l.month == now.month && l.day == now.day, isTrue,
          reason: 'every row in the day-view is from the selected day');
    }

    final longAgo = await api.captures(day: DateTime(2000, 1, 1));
    expect(longAgo, isEmpty, reason: 'a day with no captures returns nothing');

    final live = await api.captures();
    expect(live.any((i) => i.body == 'today thought'), isTrue,
        reason: 'the unscoped live feed still returns everything');
  });
}
