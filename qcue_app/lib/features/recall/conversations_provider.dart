// QCue REC-R8: the recall history drawer's state. Loads the tenant's conversations (newest first)
// through the single QcueApiClient seam, modeled as the sealed ScreenState 4-state machine (S4-R3).
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/recall_conversation.dart';
import '../../core/models/screen_state.dart';
import '../../core/net/api_client_provider.dart';
import '../../core/sync/cache_revision.dart';

final conversationsProvider =
    FutureProvider<ScreenState<List<ConversationSummary>>>((ref) async {
  // Re-read when a recall turn finishes so a newly-created thread appears in the
  // history drawer without an app relaunch (REC-R8).
  ref.watch(cacheRevisionProvider);
  final rows = await ref.watch(apiClientProvider).listConversations();
  return rows.isEmpty ? const Empty() : Data(rows);
});
