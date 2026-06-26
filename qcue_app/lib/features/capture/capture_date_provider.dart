// QCue: the Capture screen's "browse by day" state. A calendar button lets the user pick any date and
// see ALL of that day's captures; clearing returns to the live (newest-first) feed. Kept as an ADDITION
// — the default feed is unchanged.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/protocol_models.dart';
import '../../core/models/screen_state.dart';
import '../../core/net/api_client_provider.dart';
import '../../core/sync/cache_revision.dart';

/// The LOCAL calendar day the Capture screen is showing, or null for the live feed.
class SelectedCaptureDate extends Notifier<DateTime?> {
  @override
  DateTime? build() => null;

  /// Show the captures for [day] (normalised to its local calendar day).
  void select(DateTime day) => state = DateTime(day.year, day.month, day.day);

  /// Return to the live (newest-first) feed.
  void clear() => state = null;
}

final selectedCaptureDateProvider =
    NotifierProvider<SelectedCaptureDate, DateTime?>(SelectedCaptureDate.new);

/// All captures for a chosen LOCAL day, via the date-scoped seam. Re-reads on a cache-revision bump
/// (so a sync that lands a capture for the viewed day surfaces without leaving the day view).
/// autoDispose so off-screen days (the screen watches exactly one at a time) are reclaimed and stop
/// re-fetching on every bump — the date key is unbounded over wall-clock days.
final dayCapturesProvider =
    FutureProvider.autoDispose.family<ScreenState<List<Idea>>, DateTime>((ref, day) async {
  ref.watch(cacheRevisionProvider);
  final feed = await ref.watch(apiClientProvider).captures(day: day);
  return feed.isEmpty ? const Empty() : Data(feed);
});
