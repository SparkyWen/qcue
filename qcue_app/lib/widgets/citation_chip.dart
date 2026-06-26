// QCue S4-R39: a subtle footnote chip for a [Citation] — JetBrains Mono on the
// `info` token, tappable, with an accessible label.
import 'package:flutter/material.dart';
import '../core/models/sse_event.dart';
import '../core/theme/qcue_space.dart';
import '../core/theme/qcue_text.dart';
import '../core/theme/qcue_theme.dart';

class CitationChip extends StatelessWidget {
  const CitationChip({super.key, required this.citation, required this.onTap});
  final Citation citation;
  final void Function(Citation) onTap;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      label: 'citation, ${citation.relPath} line ${citation.startLine}',
      child: InkWell(
        onTap: () => onTap(citation),
        borderRadius: BorderRadius.circular(QRadius.input),
        child: Container(
          constraints: const BoxConstraints(minHeight: 44),
          padding: const EdgeInsets.symmetric(
            horizontal: QSpace.sm,
            vertical: QSpace.xs,
          ),
          decoration: BoxDecoration(
            border: Border.all(color: context.q.border),
            borderRadius: BorderRadius.circular(QRadius.input),
          ),
          child: Center(
            widthFactor: 1,
            child: Text(
              citation.label,
              style: QCueText.mono.copyWith(color: context.q.info),
            ),
          ),
        ),
      ),
    );
  }
}
