// QCue S4-R38 / D18: a collapsed-by-default disclosure for the agent's
// reasoning. The reasoning is never shown unless the reader opts in — recall
// answers lead with the conclusion, not the chain of thought. Renders nothing
// when there is no reasoning to show.
import 'package:flutter/material.dart';
import '../core/theme/qcue_space.dart';
import '../core/theme/qcue_text.dart';
import '../core/theme/qcue_theme.dart';

class ReasoningDisclosure extends StatefulWidget {
  const ReasoningDisclosure({super.key, required this.reasoning});

  final String reasoning;

  @override
  State<ReasoningDisclosure> createState() => _ReasoningDisclosureState();
}

class _ReasoningDisclosureState extends State<ReasoningDisclosure> {
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    if (widget.reasoning.trim().isEmpty) return const SizedBox.shrink();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Semantics(
          button: true,
          expanded: _expanded,
          label: 'Reasoning, ${_expanded ? 'expanded' : 'collapsed'}',
          child: InkWell(
            onTap: () => setState(() => _expanded = !_expanded),
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: QSpace.xs),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(
                    _expanded ? Icons.expand_less : Icons.expand_more,
                    size: 16,
                    color: context.q.text2,
                  ),
                  const SizedBox(width: QSpace.xs),
                  Text('Reasoning',
                      style:
                          QCueText.caption.copyWith(color: context.q.text2)),
                ],
              ),
            ),
          ),
        ),
        if (_expanded)
          Padding(
            padding: const EdgeInsets.only(top: QSpace.xs, bottom: QSpace.sm),
            child: Text(
              widget.reasoning,
              style: QCueText.caption.copyWith(color: context.q.text2),
            ),
          ),
      ],
    );
  }
}
