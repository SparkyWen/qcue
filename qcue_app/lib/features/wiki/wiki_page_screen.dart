// QCue S4-R34/R57/R63: the Wiki page view. Renders the page's markdown body
// (via MarkdownView, with inline [[wikilinks]] that route to their slug), a
// quiet metadata line (type · updated · backlink count), and a Backlinks
// section listing pages that link here. Page-not-found state for a missing
// slug. Content-first, generous whitespace, hairline dividers.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/protocol_models.dart';
import '../../core/models/screen_state.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import '../../widgets/empty_state.dart';
import '../../widgets/markdown_view.dart';
import 'wiki_provider.dart';

class WikiPageScreen extends ConsumerWidget {
  const WikiPageScreen({
    super.key,
    required this.slug,
    this.onOpenPage,
    this.onOpenConnections,
  });

  final String slug;
  final void Function(String slug)? onOpenPage;

  /// Opens the 1-hop connections view for this page (S4-R36).
  final VoidCallback? onOpenConnections;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final async = ref.watch(wikiPageProvider(slug));
    return async.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => _PageError(e.toString()),
      data: (state) => switch (state) {
        Empty() => const EmptyState(
            key: ValueKey('wiki-page-not-found'),
            icon: Icons.help_outline,
            title: "This page hasn't been written yet",
            hint: 'It may appear after the next consolidation.',
          ),
        Loading() => const Center(child: CircularProgressIndicator()),
        ErrorState(:final message) => _PageError(message),
        Data(:final value) => _PageBody(
            page: value,
            onOpenPage: onOpenPage,
            onOpenConnections: onOpenConnections,
          ),
      },
    );
  }
}

class _PageBody extends StatelessWidget {
  const _PageBody({
    required this.page,
    required this.onOpenPage,
    this.onOpenConnections,
  });
  final WikiPage page;
  final void Function(String slug)? onOpenPage;
  final VoidCallback? onOpenConnections;

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(QSpace.md),
      children: [
        Text(page.title,
            style: QCueText.title.copyWith(color: context.q.text)),
        const SizedBox(height: QSpace.xs),
        Text(
          key: const ValueKey('wiki-meta'),
          _meta(page),
          style: QCueText.caption.copyWith(color: context.q.text3),
        ),
        const SizedBox(height: QSpace.md),
        Divider(height: 1, color: context.q.border),
        const SizedBox(height: QSpace.md),
        MarkdownView(page.bodyMarkdown, onTapLink: onOpenPage),
        if (onOpenConnections != null) ...[
          const SizedBox(height: QSpace.md),
          Semantics(
            button: true,
            label: 'view connections',
            child: InkWell(
              key: const ValueKey('open-connections'),
              onTap: onOpenConnections,
              borderRadius: BorderRadius.circular(QRadius.control),
              child: ConstrainedBox(
                constraints: const BoxConstraints(minHeight: 44),
                child: Row(
                  children: [
                    Icon(Icons.hub_outlined, size: 18, color: context.q.accent),
                    const SizedBox(width: QSpace.sm),
                    Text('View connections',
                        style:
                            QCueText.body.copyWith(color: context.q.linkText)),
                  ],
                ),
              ),
            ),
          ),
        ],
        const SizedBox(height: QSpace.lg),
        _Backlinks(backlinks: page.backlinks, onOpenPage: onOpenPage),
      ],
    );
  }

  static String _meta(WikiPage p) {
    final n = p.backlinks.length;
    // p.updated is stored UTC; show the user's LOCAL edit date.
    final lu = p.updated.toLocal();
    final updated = '${lu.year}-${lu.month.toString().padLeft(2, '0')}'
        '-${lu.day.toString().padLeft(2, '0')}';
    return '${wikiPageTypeLabel(p.type)} · updated $updated · '
        '$n backlink${n == 1 ? '' : 's'}';
  }
}

class _Backlinks extends StatelessWidget {
  const _Backlinks({required this.backlinks, required this.onOpenPage});
  final List<WikiLink> backlinks;
  final void Function(String slug)? onOpenPage;

  @override
  Widget build(BuildContext context) {
    if (backlinks.isEmpty) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Backlinks',
              style: QCueText.label.copyWith(color: context.q.text)),
          const SizedBox(height: QSpace.xs),
          Text('No pages link here yet.',
              style: QCueText.caption.copyWith(color: context.q.text3)),
        ],
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Backlinks',
            style: QCueText.label.copyWith(color: context.q.text)),
        const SizedBox(height: QSpace.xs),
        for (final b in backlinks)
          Semantics(
            link: true,
            label: 'backlink, ${b.display ?? b.targetSlug}',
            child: InkWell(
              key: ValueKey('backlink-${b.targetSlug}'),
              onTap: () => onOpenPage?.call(b.targetSlug),
              child: ConstrainedBox(
                constraints: const BoxConstraints(minHeight: 44),
                child: Align(
                  alignment: Alignment.centerLeft,
                  child: Text(
                    b.display ?? b.targetSlug,
                    style: QCueText.body.copyWith(color: context.q.linkText),
                  ),
                ),
              ),
            ),
          ),
      ],
    );
  }
}

class _PageError extends StatelessWidget {
  const _PageError(this.message);
  final String message;
  @override
  Widget build(BuildContext context) => Center(
        child: Padding(
          padding: const EdgeInsets.all(QSpace.xl),
          child: Text("Couldn't load this page · $message",
              textAlign: TextAlign.center,
              style: QCueText.body.copyWith(color: context.q.danger)),
        ),
      );
}
