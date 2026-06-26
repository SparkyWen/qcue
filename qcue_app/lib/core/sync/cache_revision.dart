// QCue: the cache-revision signal. A monotonically increasing counter bumped whenever the offline
// cache (or server-derived state) changes out-of-band of a user action on the current screen:
//   - a read-sync `pull()` applied a delta (another device's capture, a digested wiki page),
//   - a recall turn finished (a new conversation + possible wiki/curated-memory edits).
//
// The read-providers that render that state (`captureFeedProvider`, `wikiIndexProvider`,
// `wikiPageProvider`, `conversationsProvider`) `ref.watch(cacheRevisionProvider)` so they RE-READ
// when it bumps. This fixes the "I have to restart the app to see digest/recall results" staleness
// (every read-provider built once and then cached its value until the next launch) AND the
// first-open-blank race: a cold provider that resolved against an empty cache re-resolves when the
// first sync snapshot bumps this — no relaunch needed.
import 'package:flutter_riverpod/flutter_riverpod.dart';

class CacheRevision extends Notifier<int> {
  @override
  int build() => 0;

  /// Signal that cached/server-derived state changed; watchers re-read.
  void bump() => state = state + 1;
}

final cacheRevisionProvider =
    NotifierProvider<CacheRevision, int>(CacheRevision.new);
