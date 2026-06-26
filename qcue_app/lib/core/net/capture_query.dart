// QCue: helpers for date-scoped capture queries. A "day" is always a LOCAL calendar day (what the user
// sees in the feed's Today/Yesterday/date grouping); the wire + cache filter use the half-open UTC
// window [start, end) for that local day so the server never has to assume a timezone.

/// Whether [instant] falls on the LOCAL calendar day [day].
bool sameLocalDay(DateTime instant, DateTime day) {
  final l = instant.toLocal();
  return l.year == day.year && l.month == day.month && l.day == day.day;
}

/// The half-open UTC window `[start, end)` covering the LOCAL calendar day [day]. The end is the NEXT
/// local midnight (not start + 24h) so a DST transition day is the correct 23h/25h window and agrees
/// with [sameLocalDay]; DateTime normalises month/year rollover (Dec 31 → Jan 1).
(DateTime, DateTime) utcDayBounds(DateTime day) {
  final start = DateTime(day.year, day.month, day.day); // local midnight
  final end = DateTime(day.year, day.month, day.day + 1); // next local midnight
  return (start.toUtc(), end.toUtc());
}
