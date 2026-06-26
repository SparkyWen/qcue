// QCue S4-R30/R31: the Capture feed state. Reads the reverse-chronological feed
// through the single QcueApiClient seam; `commit` persists a new capture
// (offline-first via the client) and optimistically refreshes the feed. Modeled
// as the sealed ScreenState 4-state machine (S4-R3).
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/protocol_models.dart';
import '../../core/models/screen_state.dart';
import '../../core/net/api_client_provider.dart';
import '../../core/sync/cache_revision.dart';

class CaptureFeedNotifier extends AsyncNotifier<ScreenState<List<Idea>>> {
  @override
  Future<ScreenState<List<Idea>>> build() async {
    // Re-read whenever a sync pull / recall lands new data, and self-heal a cold
    // first-open blank once the first snapshot arrives (no relaunch needed).
    ref.watch(cacheRevisionProvider);
    final feed = await ref.watch(apiClientProvider).captures();
    return feed.isEmpty ? const Empty() : Data(feed);
  }

  /// Persist a new capture (state `pending`) and refresh the feed.
  Future<Idea> commit({required String body, required String origin}) async {
    final api = ref.read(apiClientProvider);
    final idea = await api.capture(body: body, origin: origin);
    final feed = await api.captures();
    state = AsyncData(feed.isEmpty ? const Empty() : Data(feed));
    return idea;
  }

  /// Pull-to-refresh / retry.
  Future<void> refresh() async {
    state = const AsyncLoading();
    state = await AsyncValue.guard(() async {
      final feed = await ref.read(apiClientProvider).captures();
      return feed.isEmpty ? const Empty() : Data(feed);
    });
  }

  /// Edit a capture then refresh the feed + its detail (CAP-R2).
  Future<void> editCapture(String id, {String? body, double? lat, double? lng}) async {
    await ref.read(apiClientProvider).updateCapture(id, body: body, lat: lat, lng: lng);
    ref.invalidate(captureDetailProvider(id));
    await refresh();
  }

  /// Delete a capture then refresh the feed (CAP-R3).
  Future<void> removeCapture(String id) async {
    await ref.read(apiClientProvider).deleteCapture(id);
    ref.invalidate(captureDetailProvider(id));
    await refresh();
  }
}

final captureFeedProvider =
    AsyncNotifierProvider<CaptureFeedNotifier, ScreenState<List<Idea>>>(
        CaptureFeedNotifier.new);

/// One capture's detail by id (CAP-R1); `Empty` when not found / deleted. Re-reads on a cache bump.
final captureDetailProvider =
    FutureProvider.family<ScreenState<Idea>, String>((ref, id) async {
  ref.watch(cacheRevisionProvider);
  final idea = await ref.watch(apiClientProvider).captureDetail(id);
  return idea == null ? const Empty() : Data(idea);
});
