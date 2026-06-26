import '../models/protocol_models.dart';

/// Count ideas whose `capturedAt` falls on the USER's local calendar day as of [localNow].
///
/// `capturedAt` is stored in UTC (unified across devices for sync); "today" for the home-screen widget
/// is a LOCAL-day bucket — NOT a rolling 24h window. A 24h window miscounts near midnight (it keeps an
/// idea from 23:00 "yesterday" for an hour into today, and drops a 02:00-today idea the next morning
/// mid-day). Comparing local calendar fields is the correct, timezone-aware semantics.
int todaysLocalCount(Iterable<Idea> ideas, DateTime localNow) {
  final n = localNow.toLocal();
  return ideas.where((i) {
    final l = i.capturedAt.toLocal();
    return l.year == n.year && l.month == n.month && l.day == n.day;
  }).length;
}
