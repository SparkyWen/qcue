// QCue S4: Wiki state. The index loads the full page list (title + summary) and
// each page view loads one page by slug — both through the single QcueApiClient
// seam. Modeled as the sealed ScreenState 4-state machine (S4-R3): a null page
// for a known-missing slug surfaces as the page-not-found state.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/protocol_models.dart';
import '../../core/models/screen_state.dart';
import '../../core/net/api_client_provider.dart';
import '../../core/sync/cache_revision.dart';

/// The wiki index (every page, body-less). Re-reads when a sync pull lands freshly
/// digested pages (so the one-click Digest surfaces without an app relaunch).
final wikiIndexProvider =
    FutureProvider<ScreenState<List<WikiPage>>>((ref) async {
  ref.watch(cacheRevisionProvider);
  final pages = await ref.watch(apiClientProvider).wikiIndex();
  return pages.isEmpty ? const Empty() : Data(pages);
});

/// A single wiki page by slug; `Empty` (page-not-found) when the seam returns
/// null. Re-reads on a cache-revision bump so an open page reflects a synced edit.
final wikiPageProvider =
    FutureProvider.family<ScreenState<WikiPage>, String>((ref, slug) async {
  ref.watch(cacheRevisionProvider);
  final page = await ref.watch(apiClientProvider).wikiPage(slug);
  return page == null ? const Empty() : Data(page);
});
