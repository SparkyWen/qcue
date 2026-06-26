// QCue S4-R31/R32: the capture feed — a virtualized, reverse-chronological
// outline grouped by day. Each row: a small ingest-status dot, a subtle relative
// timestamp, and the plain capture text. ListView.builder so only on-screen rows
// build. Hairline dividers, generous whitespace, content-first.
import 'package:flutter/material.dart';
import '../../../core/models/protocol_models.dart';
import '../../../core/theme/qcue_space.dart';
import '../../../core/theme/qcue_text.dart';
import '../../../core/theme/qcue_theme.dart';
import '../../../widgets/status_dot.dart';

class FeedList extends StatelessWidget {
  const FeedList(
      {super.key, required this.ideas, this.now, this.onRetry, this.onOpen});

  final List<Idea> ideas;
  final DateTime? now;

  /// S4-R33: re-submit a failed capture (a fresh attempt). Null ⇒ no retry
  /// affordance.
  final void Function(Idea idea)? onRetry;

  /// CAP-R1: open a row's detail view. Null ⇒ rows aren't tappable.
  final void Function(Idea idea)? onOpen;

  @override
  Widget build(BuildContext context) {
    final clock = now ?? DateTime.now();
    final rows = _withDayHeaders(ideas, clock);
    return ListView.builder(
      padding: const EdgeInsets.only(bottom: QSpace.md),
      itemCount: rows.length,
      itemBuilder: (context, i) {
        final r = rows[i];
        if (r is _Header) {
          return Padding(
            padding: const EdgeInsets.fromLTRB(
                QSpace.md, QSpace.md, QSpace.md, QSpace.xs),
            child: Text(r.label,
                style: QCueText.caption.copyWith(color: context.q.text2)),
          );
        }
        final idea = (r as _Row).idea;
        final stateLabel = idea.queued ? 'queued' : idea.ingestState.name;
        return Semantics(
          label: '${_relative(idea.capturedAt, clock)}, '
              '$stateLabel: ${idea.body}',
          // CAP-R1: the whole row is tappable to open the capture's detail.
          child: InkWell(
            key: ValueKey('feed-row-${idea.id}'),
            onTap: onOpen == null ? null : () => onOpen!(idea),
            child: Padding(
            padding: const EdgeInsets.symmetric(
                horizontal: QSpace.md, vertical: QSpace.sm),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Padding(
                  padding: const EdgeInsets.only(top: 7, right: QSpace.sm),
                  child: ExcludeSemantics(
                    child: StatusDot(
                        state: idea.ingestState, queued: idea.queued),
                  ),
                ),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(idea.body,
                          style:
                              QCueText.body.copyWith(color: context.q.text)),
                      const SizedBox(height: 2),
                      Text(_relative(idea.capturedAt, clock),
                          style: QCueText.caption
                              .copyWith(color: context.q.text3)),
                      // S4-R33: a failed capture is retryable (re-submits the
                      // body as a fresh attempt). ≥44pt tap target.
                      if (idea.ingestState == IngestState.failed &&
                          onRetry != null)
                        Align(
                          alignment: Alignment.centerLeft,
                          child: TextButton.icon(
                            key: ValueKey('retry-${idea.id}'),
                            onPressed: () => onRetry!(idea),
                            icon: const Icon(Icons.refresh, size: 16),
                            label: const Text('Retry'),
                            style: TextButton.styleFrom(
                              foregroundColor: context.q.danger,
                              minimumSize: const Size(0, 44),
                              padding: const EdgeInsets.symmetric(
                                  horizontal: QSpace.sm),
                            ),
                          ),
                        ),
                    ],
                  ),
                ),
              ],
            ),
          ),
          ),
        );
      },
    );
  }

  /// A subtle relative timestamp ("just now", "5m", "2h", "yesterday", date). `difference()` compares
  /// absolute instants (zone-correct), but the absolute-date fallback prints the user's LOCAL date.
  static String _relative(DateTime t, DateTime now) {
    final d = now.difference(t);
    if (d.inMinutes < 1) return 'just now';
    if (d.inMinutes < 60) return '${d.inMinutes}m';
    if (d.inHours < 24) return '${d.inHours}h';
    if (d.inDays == 1) return 'yesterday';
    if (d.inDays < 7) return '${d.inDays}d';
    final lt = t.toLocal();
    return '${lt.year}-${lt.month.toString().padLeft(2, '0')}-'
        '${lt.day.toString().padLeft(2, '0')}';
  }

  // capturedAt is stored in UTC (unified across devices); the feed groups/labels by the USER's LOCAL
  // calendar day, so convert to local BEFORE bucketing or the day rolls over at the wrong instant.
  static List<Object> _withDayHeaders(List<Idea> ideas, DateTime now) {
    final out = <Object>[];
    String? lastDay;
    for (final i in ideas) {
      final lt = i.capturedAt.toLocal();
      final day = '${lt.year}-${lt.month}-${lt.day}';
      if (day != lastDay) {
        out.add(_Header(_dayLabel(i.capturedAt, now)));
        lastDay = day;
      }
      out.add(_Row(i));
    }
    return out;
  }

  static String _dayLabel(DateTime t, DateTime now) {
    final nl = now.toLocal();
    final tl = t.toLocal();
    final today = DateTime(nl.year, nl.month, nl.day);
    final that = DateTime(tl.year, tl.month, tl.day);
    final diff = today.difference(that).inDays;
    if (diff == 0) return 'Today';
    if (diff == 1) return 'Yesterday';
    return '${tl.year}-${tl.month.toString().padLeft(2, '0')}-'
        '${tl.day.toString().padLeft(2, '0')}';
  }
}

class _Header {
  _Header(this.label);
  final String label;
}

class _Row {
  _Row(this.idea);
  final Idea idea;
}
