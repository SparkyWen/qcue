// QCue S4: the Wiki index — a searchable list of pages grouped by
// wiki_page_type, each row a title + one-line summary. Tapping a row opens that
// page's slug (deep-link via [onOpenPage]). Content-first, hairline dividers,
// generous whitespace; loading/empty/error states.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/protocol_models.dart';
import '../../core/models/screen_state.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import '../../widgets/empty_state.dart';
import '../../widgets/skeleton.dart';
import 'wiki_provider.dart';

class WikiScreen extends ConsumerStatefulWidget {
  const WikiScreen({super.key, this.onOpenPage});

  /// Opens a page by slug (router deep-link in the app; spy in tests).
  final void Function(String slug)? onOpenPage;

  @override
  ConsumerState<WikiScreen> createState() => _WikiScreenState();
}

class _WikiScreenState extends ConsumerState<WikiScreen> {
  String _query = '';

  @override
  Widget build(BuildContext context) {
    final async = ref.watch(wikiIndexProvider);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.all(QSpace.md),
          child: TextField(
            key: const ValueKey('wiki-search'),
            onChanged: (v) => setState(() => _query = v.trim().toLowerCase()),
            style: QCueText.body.copyWith(color: context.q.text),
            decoration: InputDecoration(
              hintText: 'Search pages…',
              hintStyle: QCueText.body.copyWith(color: context.q.text3),
              prefixIcon: Icon(Icons.search, color: context.q.text3, size: 20),
              filled: true,
              fillColor: context.q.surface,
              border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(QRadius.control),
                borderSide: BorderSide(color: context.q.border),
              ),
              enabledBorder: OutlineInputBorder(
                borderRadius: BorderRadius.circular(QRadius.control),
                borderSide: BorderSide(color: context.q.border),
              ),
            ),
          ),
        ),
        Expanded(
          child: async.when(
            loading: () => const DelayedSkeleton(child: SkeletonList()),
            error: (e, _) => _WikiError(e.toString()),
            data: (state) => switch (state) {
              Empty() => const EmptyState(
                  key: ValueKey('wiki-empty'),
                  icon: Icons.menu_book_outlined,
                  title: 'No pages yet',
                  hint: 'Pages appear as your captures consolidate.',
                ),
              Loading() => const DelayedSkeleton(child: SkeletonList()),
              ErrorState(:final message) => _WikiError(message),
              Data(:final value) => _WikiList(
                  pages: _filter(value),
                  onOpenPage: widget.onOpenPage,
                  empty: _query.isNotEmpty,
                ),
            },
          ),
        ),
      ],
    );
  }

  List<WikiPage> _filter(List<WikiPage> pages) {
    if (_query.isEmpty) return pages;
    return pages
        .where((p) =>
            p.title.toLowerCase().contains(_query) ||
            p.summary.toLowerCase().contains(_query) ||
            p.tags.any((t) => t.toLowerCase().contains(_query)))
        .toList();
  }
}

class _WikiList extends StatelessWidget {
  const _WikiList({
    required this.pages,
    required this.onOpenPage,
    required this.empty,
  });

  final List<WikiPage> pages;
  final void Function(String slug)? onOpenPage;
  final bool empty;

  @override
  Widget build(BuildContext context) {
    if (pages.isEmpty) {
      return Center(
        child: Text('No matching pages',
            style: QCueText.body.copyWith(color: context.q.text3)),
      );
    }
    // Group by type, stable order, header per group.
    final grouped = <WikiPageType, List<WikiPage>>{};
    for (final p in pages) {
      (grouped[p.type] ??= []).add(p);
    }
    final rows = <Widget>[];
    for (final type in WikiPageType.values) {
      final group = grouped[type];
      if (group == null || group.isEmpty) continue;
      rows.add(Padding(
        padding:
            const EdgeInsets.fromLTRB(QSpace.md, QSpace.md, QSpace.md, QSpace.xs),
        child: Text(wikiPageTypeLabel(type).toUpperCase(),
            style: QCueText.caption.copyWith(
              color: context.q.text2,
              letterSpacing: 0.6,
            )),
      ));
      for (final p in group) {
        rows.add(_WikiRow(page: p, onOpenPage: onOpenPage));
        rows.add(Divider(
            height: 1, indent: QSpace.md, color: context.q.border));
      }
    }
    return ListView(children: rows);
  }
}

class _WikiRow extends StatelessWidget {
  const _WikiRow({required this.page, required this.onOpenPage});
  final WikiPage page;
  final void Function(String slug)? onOpenPage;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      label: '${page.title}. ${page.summary}',
      child: InkWell(
        onTap: () => onOpenPage?.call(page.slug),
        child: ConstrainedBox(
          constraints: const BoxConstraints(minHeight: 44),
          child: Padding(
            padding: const EdgeInsets.symmetric(
                horizontal: QSpace.md, vertical: QSpace.sm),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(page.title,
                    style: QCueText.label.copyWith(color: context.q.text)),
                if (page.summary.isNotEmpty) ...[
                  const SizedBox(height: 2),
                  Text(page.summary,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style:
                          QCueText.caption.copyWith(color: context.q.text2)),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _WikiError extends StatelessWidget {
  const _WikiError(this.message);
  final String message;
  @override
  Widget build(BuildContext context) => Center(
        child: Padding(
          padding: const EdgeInsets.all(QSpace.xl),
          child: Text("Couldn't load the wiki · $message",
              textAlign: TextAlign.center,
              style: QCueText.body.copyWith(color: context.q.danger)),
        ),
      );
}
