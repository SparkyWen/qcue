// QCue S4-R43 / A-R38: the Dream-completion feed entry — "Improved N pages",
// mirroring Claude's inline "Improved N files" (verb 'Improved'). An optional
// needs-review badge (pending token) shows when the Dream produced confirm-
// required merges/deletes. Tapping routes to the candidate diff.
import 'package:flutter/material.dart';
import '../../../core/theme/qcue_space.dart';
import '../../../core/theme/qcue_text.dart';
import '../../../core/theme/qcue_theme.dart';

class ImprovedPagesEntry extends StatelessWidget {
  const ImprovedPagesEntry({
    super.key,
    required this.pagesImproved,
    required this.needsReview,
    required this.onTap,
  });

  final int pagesImproved;
  final bool needsReview;
  final VoidCallback onTap;

  String get _label =>
      'Improved $pagesImproved ${pagesImproved == 1 ? 'page' : 'pages'}';

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      label: '$_label${needsReview ? ', needs review' : ''}',
      child: InkWell(
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.symmetric(
              horizontal: QSpace.md, vertical: QSpace.sm),
          child: Row(
            children: [
              Icon(Icons.auto_awesome, size: 16, color: context.q.success),
              const SizedBox(width: QSpace.sm),
              Text(_label, style: QCueText.body.copyWith(color: context.q.text)),
              const Spacer(),
              if (needsReview)
                Container(
                  key: const ValueKey('needs-review-badge'),
                  padding: const EdgeInsets.symmetric(
                      horizontal: QSpace.sm, vertical: 2),
                  decoration: BoxDecoration(
                    color: context.q.pending,
                    borderRadius: BorderRadius.circular(QRadius.input),
                  ),
                  child: Text('Needs review',
                      style: QCueText.caption
                          .copyWith(color: context.q.bg, fontSize: 11)),
                ),
              const SizedBox(width: QSpace.xs),
              Icon(Icons.chevron_right, size: 18, color: context.q.text3),
            ],
          ),
        ),
      ),
    );
  }
}
