// QCue DIG-R6: the one-click incremental Digest action. Calls the runIngest() seam (POST /v1/ingest/run)
// and tracks an idle → running → done(count)|failed lifecycle so the trigger (now a Settings row) can
// disable while in flight and show the enqueued count.
//
// Lives in core/ (not features/wiki) so the Settings screen can drive it without a cross-feature import
// (S4-R4). It deliberately does NOT invalidate the wiki index: ingest runs asynchronously server-side,
// so the freshly-digested pages land on this device via the read-sync pull, which bumps the cache
// revision and re-reads wikiIndexProvider — surfacing the result without a relaunch (see cache_revision).
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../net/api_client_provider.dart';

/// The digest action lifecycle (sealed so the row matches exhaustively).
sealed class DigestState {
  const DigestState();
}

class DigestIdle extends DigestState {
  const DigestIdle();
}

class DigestRunning extends DigestState {
  const DigestRunning();
}

class DigestDone extends DigestState {
  const DigestDone(this.enqueued);
  final int enqueued;
}

class DigestFailed extends DigestState {
  const DigestFailed(this.message);
  final String message;
}

class DigestNotifier extends Notifier<DigestState> {
  @override
  DigestState build() => const DigestIdle();

  /// Enqueue an ingest job per new/edited capture. The digested pages surface via the
  /// read-sync pull → cache-revision bump (no premature, empty wiki re-fetch here).
  Future<void> run() async {
    if (state is DigestRunning) return; // guard against double-tap re-entry
    state = const DigestRunning();
    try {
      final enqueued = await ref.read(apiClientProvider).runIngest();
      state = DigestDone(enqueued);
    } catch (e) {
      state = DigestFailed(e.toString());
    }
  }
}

final digestProvider =
    NotifierProvider<DigestNotifier, DigestState>(DigestNotifier.new);
