// QCue S4-R30/R32: a small ingest-status dot. Its color maps from the idea's
// `ingest_state` onto semantic tokens; it carries an accessible label so the
// state is never color-only.
import 'package:flutter/material.dart';
import '../core/models/protocol_models.dart';
import '../core/theme/qcue_motion.dart';
import '../core/theme/qcue_theme.dart';

class StatusDot extends StatelessWidget {
  const StatusDot({
    super.key,
    required this.state,
    this.size = 8,
    this.queued = false,
  });

  final IngestState state;
  final double size;

  /// True for a locally-queued (offline) capture: renders a distinct HOLLOW
  /// dot in the pending hue so it reads differently from a server-pending row,
  /// and announces "queued, will sync" (never color-only — S4-R32).
  final bool queued;

  Color _color(QCueColors q) => switch (state) {
        IngestState.pending => q.pending,
        IngestState.ingesting => q.info,
        IngestState.ingested => q.success,
        IngestState.skippedRedundant => q.text3,
        IngestState.failed => q.danger,
      };

  String get _label => queued
      ? 'queued, will sync'
      : switch (state) {
          IngestState.pending => 'pending',
          IngestState.ingesting => 'ingesting',
          IngestState.ingested => 'ingested',
          IngestState.skippedRedundant => 'skipped, redundant',
          IngestState.failed => 'failed to ingest',
        };

  @override
  Widget build(BuildContext context) {
    final c = _color(context.q);
    return Semantics(
      label: 'status, $_label',
      // S4-R32: the dot cross-fades its color IN PLACE when the ingest state
      // changes (e.g. pending→ingested), 150-300ms, collapsed to instant under
      // reduced motion (S4-R61). Finite implicit animation — it settles, so it
      // is widget-test-safe (no perpetual pulse). Filled vs hollow (queued)
      // keeps the state non-color-only.
      child: AnimatedContainer(
        duration: QMotion.durationOrZero(context, QMotion.base),
        curve: QMotion.enter,
        width: size,
        height: size,
        decoration: BoxDecoration(
          // Queued = hollow ring; otherwise a filled dot.
          color: queued ? null : c,
          border: queued ? Border.all(color: c, width: 1.5) : null,
          shape: BoxShape.circle,
        ),
      ),
    );
  }
}
