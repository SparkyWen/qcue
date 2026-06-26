// QCue S4-R42 / A-R37: replicates DreamDetailDialog. Title "Memory
// consolidation"; live subtitle (elapsed · reviewing N sessions · M pages
// touched); status colored via design tokens; the LAST VISIBLE_TURNS=6 turns
// with tool-uses collapsed to a dim count, earlier ones collapsed to
// "(K earlier turns)"; pages-touched ("at least these"); a Cancel mapping to
// DreamTask.kill → clock rollback (A-R8). Reasoning collapsed-by-default (D18).
import 'package:flutter/material.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import '../../core/theme/qcue_tokens.dart';
import '../../widgets/reasoning_disclosure.dart';
import 'dream_provider.dart';

const _visibleTurns = 6; // DreamTask.ts VISIBLE_TURNS

class DreamDetailScreen extends StatelessWidget {
  const DreamDetailScreen({
    super.key,
    required this.state,
    required this.onCancel,
  });

  final DreamState state;
  final VoidCallback onCancel;

  QToken get _statusToken => switch (state.status) {
        DreamStatus.running => QToken.pending,
        DreamStatus.completed => QToken.success,
        DreamStatus.failed => QToken.danger,
      };

  String get _statusLabel => switch (state.status) {
        DreamStatus.running => 'running',
        DreamStatus.completed => 'completed',
        DreamStatus.failed => 'failed',
      };

  @override
  Widget build(BuildContext context) {
    final withText = state.turns.where((t) => t.text.isNotEmpty).toList();
    final earlier = withText.length - _visibleTurns;
    final visible = withText.length > _visibleTurns
        ? withText.sublist(withText.length - _visibleTurns)
        : withText;

    return Scaffold(
      backgroundColor: context.q.bg,
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(QSpace.md),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(state.title,
                  style: QCueText.subtitle.copyWith(color: context.q.text)),
              const SizedBox(height: QSpace.xs),
              Text(
                '${_elapsed(state.elapsed)} · reviewing '
                '${state.sessionsReviewing} sessions · '
                '${state.pagesTouched.length} pages touched',
                style: QCueText.caption.copyWith(color: context.q.text2),
              ),
              const SizedBox(height: QSpace.sm),
              Semantics(
                label: 'status, $_statusLabel',
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Container(
                      width: 8,
                      height: 8,
                      decoration: BoxDecoration(
                        color: context.q.color(_statusToken),
                        shape: BoxShape.circle,
                      ),
                    ),
                    const SizedBox(width: QSpace.sm),
                    Text(_statusLabel,
                        style: QCueText.label
                            .copyWith(color: context.q.color(_statusToken))),
                  ],
                ),
              ),
              const SizedBox(height: QSpace.sm),
              // The model's chain-of-thought, never shown unless opted in (D18).
              ReasoningDisclosure(reasoning: state.reasoning),
              const SizedBox(height: QSpace.sm),
              Expanded(
                child: ListView(
                  children: [
                    if (earlier > 0)
                      Padding(
                        padding: const EdgeInsets.symmetric(
                            vertical: QSpace.xs),
                        child: Text('($earlier earlier turns)',
                            style: QCueText.caption
                                .copyWith(color: context.q.text3)),
                      ),
                    for (final t in visible)
                      Padding(
                        padding: const EdgeInsets.symmetric(
                            vertical: QSpace.xs),
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Expanded(
                              child: Text(t.text,
                                  style: QCueText.body
                                      .copyWith(color: context.q.text)),
                            ),
                            const SizedBox(width: QSpace.sm),
                            Text('(${t.toolUseCount} tool)',
                                style: QCueText.caption
                                    .copyWith(color: context.q.text3)),
                          ],
                        ),
                      ),
                  ],
                ),
              ),
              const SizedBox(height: QSpace.sm),
              Text('Pages touched (at least):',
                  style: QCueText.caption.copyWith(color: context.q.text2)),
              const SizedBox(height: 2),
              Text(
                state.pagesTouched.isEmpty
                    ? '—'
                    : state.pagesTouched.join(' · '),
                style: QCueText.mono
                    .copyWith(color: context.q.text3, fontSize: 13),
              ),
              if (state.status == DreamStatus.running) ...[
                const SizedBox(height: QSpace.md),
                Semantics(
                  button: true,
                  label: 'Cancel the dream',
                  child: OutlinedButton.icon(
                    onPressed: onCancel,
                    style: OutlinedButton.styleFrom(
                      foregroundColor: context.q.danger,
                      side: BorderSide(color: context.q.border),
                      minimumSize: const Size.fromHeight(44),
                    ),
                    icon: const Icon(Icons.close, size: 16),
                    label: const Text('Cancel'),
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }

  static String _elapsed(Duration d) =>
      '${d.inMinutes}:${(d.inSeconds % 60).toString().padLeft(2, '0')}';
}
