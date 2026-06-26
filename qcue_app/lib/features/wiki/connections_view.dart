// QCue S4-R36: the wiki connections view. Shows the current page plus its 1-hop
// neighbors (the pages that link here — backlinks) as ≥44pt tappable nodes that
// navigate; the full graph is a labeled-disabled "coming soon" entry (M5+).
// Color-not-alone: the current node carries an accent border + a "current page"
// label, dead links are muted + non-navigating with a distinct icon.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/protocol_models.dart';
import '../../core/models/screen_state.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import '../../widgets/empty_state.dart';
import 'wiki_provider.dart';

class ConnectionsView extends ConsumerWidget {
  const ConnectionsView({super.key, required this.slug, this.onOpenPage});

  final String slug;
  final void Function(String slug)? onOpenPage;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final async = ref.watch(wikiPageProvider(slug));
    return async.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => _error(context, e.toString()),
      data: (state) => switch (state) {
        Data(:final value) => _Graph(page: value, onOpenPage: onOpenPage),
        ErrorState(:final message) => _error(context, message),
        _ => const EmptyState(
            key: ValueKey('connections-empty'),
            icon: Icons.hub_outlined,
            title: 'No connections yet',
            hint: 'Links appear as your wiki grows.',
          ),
      },
    );
  }

  Widget _error(BuildContext context, String m) => Center(
        child: Padding(
          padding: const EdgeInsets.all(QSpace.xl),
          child: Text("Couldn't load connections · $m",
              textAlign: TextAlign.center,
              style: QCueText.body.copyWith(color: context.q.danger)),
        ),
      );
}

class _Graph extends StatelessWidget {
  const _Graph({required this.page, required this.onOpenPage});
  final WikiPage page;
  final void Function(String slug)? onOpenPage;

  @override
  Widget build(BuildContext context) {
    return ListView(
      key: const ValueKey('connections-view'),
      padding: const EdgeInsets.all(QSpace.md),
      children: [
        Text('Connections',
            style: QCueText.title.copyWith(color: context.q.text)),
        const SizedBox(height: QSpace.md),
        _Node(label: page.title, isCurrent: true), // center node, non-navigating
        const SizedBox(height: QSpace.md),
        Text('Linked from (${page.backlinks.length})',
            style: QCueText.label.copyWith(color: context.q.text2)),
        const SizedBox(height: QSpace.xs),
        if (page.backlinks.isEmpty)
          Text('No pages link here yet.',
              style: QCueText.caption.copyWith(color: context.q.text3))
        else
          for (final b in page.backlinks)
            _Node(
              key: ValueKey('connection-${b.targetSlug}'),
              label: b.display ?? b.targetSlug,
              isCurrent: false,
              dead: b.isDead,
              onTap: b.isDead ? null : () => onOpenPage?.call(b.targetSlug),
            ),
        const SizedBox(height: QSpace.lg),
        const _Node(
          key: ValueKey('connections-full-graph'),
          label: 'Full graph — coming soon',
          isCurrent: false,
          disabled: true,
        ),
      ],
    );
  }
}

class _Node extends StatelessWidget {
  const _Node({
    super.key,
    required this.label,
    required this.isCurrent,
    this.onTap,
    this.dead = false,
    this.disabled = false,
  });

  final String label;
  final bool isCurrent;
  final VoidCallback? onTap;
  final bool dead;
  final bool disabled;

  @override
  Widget build(BuildContext context) {
    final muted = dead || disabled;
    final border = isCurrent ? context.q.accent : context.q.border;
    final fg = muted
        ? context.q.text3
        : (isCurrent ? context.q.accent : context.q.text);
    final icon = isCurrent
        ? Icons.my_location
        : (dead ? Icons.link_off : Icons.article_outlined);
    return Opacity(
      opacity: disabled ? 0.6 : 1,
      child: Padding(
        padding: const EdgeInsets.only(bottom: QSpace.sm),
        child: Semantics(
          button: onTap != null,
          label: isCurrent
              ? 'current page, $label'
              : (dead ? 'dead link, $label' : 'connection, $label'),
          child: InkWell(
            onTap: onTap,
            borderRadius: BorderRadius.circular(QRadius.control),
            child: ConstrainedBox(
              constraints: const BoxConstraints(minHeight: 44),
              child: Container(
                padding: const EdgeInsets.symmetric(
                    horizontal: QSpace.md, vertical: QSpace.sm),
                decoration: BoxDecoration(
                  border: Border.all(color: border, width: isCurrent ? 2 : 1),
                  borderRadius: BorderRadius.circular(QRadius.control),
                ),
                child: Row(
                  children: [
                    Icon(icon, size: 16, color: fg),
                    const SizedBox(width: QSpace.sm),
                    Expanded(
                      child: Text(label,
                          style: QCueText.body.copyWith(color: fg)),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
