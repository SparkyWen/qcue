// QCue S4: a calm, centered empty-state used by the placeholder feature screens
// (and reused by the real screens' Empty branch later). Icon + title + optional
// hint, all on semantic tokens.
import 'package:flutter/material.dart';
import '../core/theme/qcue_space.dart';
import '../core/theme/qcue_text.dart';
import '../core/theme/qcue_theme.dart';

class EmptyState extends StatelessWidget {
  const EmptyState({
    super.key,
    required this.icon,
    required this.title,
    this.hint,
  });

  final IconData icon;
  final String title;
  final String? hint;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(QSpace.xl),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 40, color: context.q.text3),
            const SizedBox(height: QSpace.md),
            Text(
              title,
              textAlign: TextAlign.center,
              style: QCueText.subtitle.copyWith(color: context.q.text),
            ),
            if (hint != null) ...[
              const SizedBox(height: QSpace.sm),
              Text(
                hint!,
                textAlign: TextAlign.center,
                style: QCueText.label.copyWith(color: context.q.text2),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
