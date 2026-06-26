// QCue REC-R8/REC-D8: the recall history left Drawer. Lists the tenant's past conversations (newest
// first) via conversationsProvider, reuses the wiki list-row pattern, renders the sealed ScreenState
// 4-state machine, and uses ONLY context.q tokens (no raw hex). "＋ new" starts a fresh thread; tapping
// a row reopens it so the next ask CONTINUES it.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/recall_conversation.dart';
import '../../core/models/screen_state.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import 'conversations_provider.dart';
import 'recall_provider.dart';

class HistoryDrawer extends ConsumerWidget {
  const HistoryDrawer({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final async = ref.watch(conversationsProvider);
    return Drawer(
      backgroundColor: context.q.surface,
      child: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // "＋ new" — mint a fresh thread (REC-D8).
            Padding(
              padding: const EdgeInsets.all(QSpace.md),
              child: InkWell(
                key: const ValueKey('recall-new-button'),
                onTap: () {
                  ref.read(recallProvider.notifier).startNew();
                  Navigator.of(context).pop();
                },
                child: Row(
                  children: [
                    Icon(Icons.add, color: context.q.accent, size: 20),
                    const SizedBox(width: QSpace.sm),
                    Text('New conversation',
                        style: QCueText.label.copyWith(color: context.q.accent)),
                  ],
                ),
              ),
            ),
            Divider(height: 1, color: context.q.border),
            Expanded(
              child: async.when(
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (e, _) => _DrawerError(e.toString()),
                data: (state) => switch (state) {
                  Loading() => const Center(child: CircularProgressIndicator()),
                  Empty() => Center(
                      child: Padding(
                        padding: const EdgeInsets.all(QSpace.xl),
                        child: Text('No past conversations yet.',
                            textAlign: TextAlign.center,
                            style: QCueText.body.copyWith(color: context.q.text3)),
                      ),
                    ),
                  ErrorState(:final message) => _DrawerError(message),
                  Data(:final value) => _ConvoList(conversations: value),
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _ConvoList extends StatelessWidget {
  const _ConvoList({required this.conversations});
  final List<ConversationSummary> conversations;

  @override
  Widget build(BuildContext context) {
    return ListView.separated(
      itemCount: conversations.length,
      separatorBuilder: (_, __) =>
          Divider(height: 1, indent: QSpace.md, color: context.q.border),
      itemBuilder: (_, i) => _ConvoRow(convo: conversations[i]),
    );
  }
}

class _ConvoRow extends ConsumerWidget {
  const _ConvoRow({required this.convo});
  final ConversationSummary convo;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Semantics(
      button: true,
      label: convo.title,
      child: InkWell(
        key: ValueKey('convo-row-${convo.id}'),
        onTap: () {
          ref.read(recallProvider.notifier).openConversation(convo.id, title: convo.title);
          Navigator.of(context).pop();
        },
        child: ConstrainedBox(
          constraints: const BoxConstraints(minHeight: 44),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: QSpace.md, vertical: QSpace.sm),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(convo.title,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: QCueText.label.copyWith(color: context.q.text)),
                if ((convo.lastSnippet ?? '').isNotEmpty) ...[
                  const SizedBox(height: 2),
                  Text(convo.lastSnippet!,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: QCueText.caption.copyWith(color: context.q.text2)),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _DrawerError extends StatelessWidget {
  const _DrawerError(this.message);
  final String message;
  @override
  Widget build(BuildContext context) => Center(
        child: Padding(
          padding: const EdgeInsets.all(QSpace.xl),
          child: Text("Couldn't load history · $message",
              textAlign: TextAlign.center,
              style: QCueText.body.copyWith(color: context.q.danger)),
        ),
      );
}
