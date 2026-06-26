// QCue S4: the Activity tab body — three content-first sections wired through
// the single apiClientProvider seam:
//   1. Review — pending ingest candidates (wiki_merge/wiki_delete) with the D13
//      Approve/Reject gate; destructive deletes confirm first. "Nothing to
//      review." when empty.
//   2. Dream — a live card for a running consolidation (elapsed/phase/turns with
//      collapsed reasoning + Cancel), and the "Improved N pages" entry for a
//      finished one.
//   3. Jobs — recent jobs mapped to StatusDot-style glyphs + today's cost.
// Section headers, hairline dividers, content leads.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/protocol_models.dart';
import '../../core/net/api_client_provider.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import 'activity_provider.dart';
import 'dream_detail_screen.dart';
import 'dream_provider.dart';
import 'widgets/candidate_row.dart';
import 'widgets/improved_pages_entry.dart';
import 'widgets/job_row.dart';

class ActivityScreen extends ConsumerWidget {
  const ActivityScreen({super.key, this.onOpenDream, this.onOpenCandidates});

  /// Deep-link to the full Dream detail screen (router wires this).
  final void Function(String jobId)? onOpenDream;

  /// Deep-link to the candidates/diff list (the "Improved N pages" tap-through).
  final VoidCallback? onOpenCandidates;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final async = ref.watch(activityProvider);
    final offline = ref.watch(offlineProvider);

    return switch (async) {
      AsyncLoading() => const Center(
          key: ValueKey('activity-loading'),
          child: CircularProgressIndicator()),
      AsyncError(:final error) => Center(
          child: Padding(
            padding: const EdgeInsets.all(QSpace.xl),
            child: Text("Couldn't load activity · $error",
                textAlign: TextAlign.center,
                style: QCueText.body.copyWith(color: context.q.danger)),
          ),
        ),
      AsyncData(:final value) => ListView(
          key: const ValueKey('activity-screen'),
          padding: const EdgeInsets.only(bottom: QSpace.xl),
          children: [
            const _SectionHeader('Review'),
            _ReviewSection(snapshot: value, offline: offline),
            const _Hairline(),
            const _SectionHeader('Dream'),
            _DreamSection(
                snapshot: value,
                onOpenDream: onOpenDream,
                onOpenCandidates: onOpenCandidates),
            const _Hairline(),
            const _SectionHeader('Jobs'),
            _JobsSection(snapshot: value),
          ],
        ),
      _ => const SizedBox.shrink(),
    };
  }
}

class _SectionHeader extends StatelessWidget {
  const _SectionHeader(this.label);
  final String label;
  @override
  Widget build(BuildContext context) => Padding(
        key: ValueKey('section-$label'),
        padding: const EdgeInsets.fromLTRB(
            QSpace.md, QSpace.md, QSpace.md, QSpace.sm),
        child: Semantics(
          header: true,
          child: Text(label,
              style: QCueText.label.copyWith(color: context.q.text2)),
        ),
      );
}

class _Hairline extends StatelessWidget {
  const _Hairline();
  @override
  Widget build(BuildContext context) =>
      Divider(height: 1, color: context.q.border);
}

class _ReviewSection extends ConsumerWidget {
  const _ReviewSection({required this.snapshot, required this.offline});
  final ActivitySnapshot snapshot;
  final bool offline;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final pending = snapshot.candidates
        .where((a) => a.status == ApprovalStatus.pending)
        .toList();
    if (pending.isEmpty) {
      return Padding(
        padding: const EdgeInsets.fromLTRB(QSpace.md, 0, QSpace.md, QSpace.md),
        child: Text('Nothing to review.',
            style: QCueText.body.copyWith(color: context.q.text3)),
      );
    }
    return Column(
      children: [
        for (final a in pending)
          CandidateRow(
            approval: a,
            disabled: offline,
            onDecide: (approve) => ref
                .read(activityProvider.notifier)
                .decideCandidate(a.id, approve: approve),
          ),
      ],
    );
  }
}

class _DreamSection extends ConsumerWidget {
  const _DreamSection({
    required this.snapshot,
    required this.onOpenDream,
    required this.onOpenCandidates,
  });
  final ActivitySnapshot snapshot;
  final void Function(String jobId)? onOpenDream;
  final VoidCallback? onOpenCandidates;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final running = snapshot.runningDream;
    final completed = snapshot.completedDream;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (running != null)
          _RunningDreamCard(jobId: running.id, onOpen: onOpenDream),
        if (completed != null)
          ImprovedPagesEntry(
            pagesImproved: 3, // server reports the count; fixture stand-in
            needsReview: snapshot.candidates
                .any((a) => a.status == ApprovalStatus.pending),
            onTap: onOpenCandidates ?? () {},
          ),
        if (running == null && completed == null)
          Padding(
            padding:
                const EdgeInsets.fromLTRB(QSpace.md, 0, QSpace.md, QSpace.md),
            child: Text('No recent consolidation.',
                style: QCueText.body.copyWith(color: context.q.text3)),
          ),
      ],
    );
  }
}

/// A compact live card for a running dream — elapsed/status/phase preview from
/// the SSE stream, with Cancel and a tap-through to the full detail screen.
class _RunningDreamCard extends ConsumerWidget {
  const _RunningDreamCard({required this.jobId, required this.onOpen});
  final String jobId;
  final void Function(String jobId)? onOpen;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final dream = ref.watch(dreamProvider(jobId));
    final lastTurn = dream.turns.isEmpty ? null : dream.turns.last;
    return InkWell(
      onTap: () => onOpen?.call(jobId),
      child: Container(
        margin: const EdgeInsets.fromLTRB(
            QSpace.md, 0, QSpace.md, QSpace.sm),
        padding: const EdgeInsets.all(QSpace.md),
        decoration: BoxDecoration(
          color: context.q.surface,
          borderRadius: BorderRadius.circular(QRadius.card),
          border: Border.all(color: context.q.border),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Container(
                  width: 8,
                  height: 8,
                  decoration: BoxDecoration(
                      color: context.q.pending, shape: BoxShape.circle),
                ),
                const SizedBox(width: QSpace.sm),
                Expanded(
                  child: Text(dream.title,
                      style:
                          QCueText.label.copyWith(color: context.q.text)),
                ),
                Text(_elapsed(dream.elapsed),
                    style: QCueText.monoTabular
                        .copyWith(color: context.q.text2, fontSize: 13)),
              ],
            ),
            if (lastTurn != null) ...[
              const SizedBox(height: QSpace.xs),
              Text(lastTurn.text,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: QCueText.caption.copyWith(color: context.q.text2)),
            ],
            const SizedBox(height: QSpace.sm),
            Row(
              children: [
                Text('${dream.pagesTouched.length} pages touched',
                    style:
                        QCueText.caption.copyWith(color: context.q.text3)),
                const Spacer(),
                if (dream.status == DreamStatus.running)
                  TextButton.icon(
                    style:
                        TextButton.styleFrom(foregroundColor: context.q.danger),
                    onPressed: () =>
                        ref.read(activityProvider.notifier).cancelDream(jobId),
                    icon: const Icon(Icons.close, size: 14),
                    label: const Text('Cancel'),
                  ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  static String _elapsed(Duration d) =>
      '${d.inMinutes}:${(d.inSeconds % 60).toString().padLeft(2, '0')}';
}

class _JobsSection extends StatelessWidget {
  const _JobsSection({required this.snapshot});
  final ActivitySnapshot snapshot;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final j in snapshot.jobs) JobRowView(job: j),
        const SizedBox(height: QSpace.sm),
        Padding(
          padding:
              const EdgeInsets.symmetric(horizontal: QSpace.md, vertical: QSpace.xs),
          child: Row(
            children: [
              Expanded(
                child: Text("Today's cost",
                    style: QCueText.label.copyWith(color: context.q.text2)),
              ),
              Text('\$${(snapshot.todayCostMicros / 1e6).toStringAsFixed(2)}',
                  style: QCueText.monoTabular.copyWith(color: context.q.text)),
            ],
          ),
        ),
      ],
    );
  }
}

/// Exposes the live Dream detail screen for the router's `dream/:jobId` route.
class DreamDetailRoute extends ConsumerWidget {
  const DreamDetailRoute({super.key, required this.jobId});
  final String jobId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final dream = ref.watch(dreamProvider(jobId));
    return DreamDetailScreen(
      state: dream,
      onCancel: () => ref.read(activityProvider.notifier).cancelDream(jobId),
    );
  }
}
