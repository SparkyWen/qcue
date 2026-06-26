// QCue S4-R41 / D13: a proposed wiki edit (candidate) from the ingest/dream
// consolidator. Each shows the proposed change — the target page slug + a short
// summary — with Approve / Reject. The decision routes through the
// candidates→confirm→canonical gate; the app never canonicalizes itself.
// Destructive deletes get a confirmation dialog (danger token) before the
// decision fires. Disabled while offline.
import 'package:flutter/material.dart';
import '../../../core/models/protocol_models.dart';
import '../../../core/theme/qcue_space.dart';
import '../../../core/theme/qcue_text.dart';
import '../../../core/theme/qcue_theme.dart';

class CandidateRow extends StatelessWidget {
  const CandidateRow({
    super.key,
    required this.approval,
    required this.onDecide,
    required this.disabled,
  });

  final Approval approval;
  final void Function(bool approve) onDecide;
  final bool disabled; // offline → disabled

  bool get _isDelete => approval.action == 'wiki_delete';

  String get _summary =>
      approval.subjectRef['summary'] as String? ?? approval.action;
  String? get _targetSlug => approval.subjectRef['target_slug'] as String?;

  Future<void> _approve(BuildContext context) async {
    // The candidates→confirm gate already makes every merge/delete explicit, but
    // a destructive DELETE additionally asks to confirm (D13 destructive guard).
    if (_isDelete) {
      final ok = await showDialog<bool>(
        context: context,
        builder: (ctx) => AlertDialog(
          title: const Text('Delete this page?'),
          content: Text(
            'This will delete ${_targetSlug ?? 'the page'}. '
            'You can restore it from the canonical history.',
            style: QCueText.body.copyWith(color: ctx.q.text2),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(ctx).pop(false),
              child: Text('Cancel',
                  style: TextStyle(color: ctx.q.text2)),
            ),
            TextButton(
              onPressed: () => Navigator.of(ctx).pop(true),
              child: Text('Delete', style: TextStyle(color: ctx.q.danger)),
            ),
          ],
        ),
      );
      if (ok == true) onDecide(true);
      return;
    }
    onDecide(true);
  }

  @override
  Widget build(BuildContext context) {
    final actionLabel = _isDelete ? 'delete' : 'merge';
    return Semantics(
      label: '$actionLabel candidate: $_summary',
      child: Padding(
        padding: const EdgeInsets.symmetric(
            horizontal: QSpace.md, vertical: QSpace.sm),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(
                  _isDelete
                      ? Icons.delete_outline
                      : Icons.merge_type_outlined,
                  size: 16,
                  color: _isDelete ? context.q.danger : context.q.pending,
                ),
                const SizedBox(width: QSpace.sm),
                Expanded(
                  child: Text(_summary,
                      style: QCueText.body.copyWith(color: context.q.text)),
                ),
              ],
            ),
            if (_targetSlug != null)
              Padding(
                padding: const EdgeInsets.only(left: 24, top: 2),
                child: Text(_targetSlug!,
                    style: QCueText.mono.copyWith(
                        color: context.q.text3, fontSize: 13)),
              ),
            Padding(
              padding: const EdgeInsets.only(left: 24, top: QSpace.xs),
              child: Row(
                children: [
                  TextButton(
                    onPressed: disabled ? null : () => onDecide(false),
                    style: TextButton.styleFrom(
                        foregroundColor: context.q.danger),
                    child: const Text('Reject'),
                  ),
                  const SizedBox(width: QSpace.xs),
                  FilledButton(
                    onPressed: disabled ? null : () => _approve(context),
                    style: FilledButton.styleFrom(
                      backgroundColor: context.q.accent,
                      foregroundColor: context.q.bg,
                    ),
                    child: const Text('Approve'),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
