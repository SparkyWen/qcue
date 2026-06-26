// QCue S4-R44: maps jobs.state → glyph + label + semantic token, read-only.
// queued/running/completed/failed/canceled/skipped, never color-only (icon +
// label + an accessible Semantics label).
import 'package:flutter/material.dart';
import '../../../core/models/protocol_models.dart';
import '../../../core/theme/qcue_space.dart';
import '../../../core/theme/qcue_text.dart';
import '../../../core/theme/qcue_theme.dart';
import '../../../core/theme/qcue_tokens.dart';

({IconData icon, String label, QToken token}) jobStateGlyph(JobState state) =>
    switch (state) {
      JobState.pending =>
        (icon: Icons.schedule, label: 'queued', token: QToken.text3),
      JobState.leased =>
        (icon: Icons.autorenew, label: 'running', token: QToken.pending),
      JobState.done => (
          icon: Icons.check_circle_outline,
          label: 'completed',
          token: QToken.success
        ),
      JobState.failed =>
        (icon: Icons.error_outline, label: 'failed', token: QToken.danger),
      JobState.skipped =>
        (icon: Icons.skip_next, label: 'skipped', token: QToken.text3),
      JobState.canceled => (
          icon: Icons.cancel_outlined,
          label: 'canceled',
          token: QToken.text3
        ),
    };

String jobKindLabel(JobKind kind) => switch (kind) {
      JobKind.ingest => 'Ingest',
      JobKind.lint => 'Lint',
      JobKind.dream => 'Dream',
      JobKind.transcribe => 'Transcribe',
      JobKind.syncMaterialize => 'Sync',
      JobKind.export => 'Export',
    };

class JobRowView extends StatelessWidget {
  const JobRowView({super.key, required this.job});
  final JobRow job;

  @override
  Widget build(BuildContext context) {
    final m = jobStateGlyph(job.state);
    final kind = jobKindLabel(job.kind);
    return Semantics(
      label: '$kind job, ${m.label}',
      child: Padding(
        padding: const EdgeInsets.symmetric(
            horizontal: QSpace.md, vertical: QSpace.sm),
        child: Row(
          children: [
            Icon(m.icon, size: 16, color: context.q.color(m.token)),
            const SizedBox(width: QSpace.sm),
            Expanded(
              child: Text(kind,
                  style: QCueText.label.copyWith(color: context.q.text)),
            ),
            Text(m.label,
                style: QCueText.caption.copyWith(color: context.q.text2)),
          ],
        ),
      ),
    );
  }
}
