// QCue S4-R14/R34: inline `[[wikilink]]` — `linkText`-colored BODY (Inter)
// text, never mono, ≥44pt hit area, routes to the slug. `linkText` (not
// `accent`) is the reading-weight link color that clears 4.5:1 AA against the
// page bg in every theme; `accent` stays reserved for CTA *fills*. Dead links
// get a distinct, non-crashing affordance.
import 'package:flutter/material.dart';
import '../core/theme/qcue_text.dart';
import '../core/theme/qcue_theme.dart';

class WikiLink extends StatelessWidget {
  const WikiLink({
    super.key,
    required this.slug,
    required this.display,
    required this.onTap,
    this.isDead = false,
  });
  final String slug;
  final String display;
  final void Function(String slug) onTap;
  final bool isDead;

  @override
  Widget build(BuildContext context) {
    final color = isDead ? context.q.text3 : context.q.linkText;
    return Semantics(
      link: true,
      label: 'link, $display${isDead ? ', not yet built' : ''}',
      child: InkWell(
        onTap: () => onTap(slug),
        child: ConstrainedBox(
          constraints: const BoxConstraints(minHeight: 44),
          child: Align(
            alignment: Alignment.centerLeft,
            child: isDead
                ? Row(
                    key: const ValueKey('dead-link'),
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(
                        display,
                        style: QCueText.body.copyWith(
                          color: color,
                          decoration: TextDecoration.lineThrough,
                        ),
                      ),
                      const SizedBox(width: 4),
                      Icon(Icons.help_outline, size: 14, color: color),
                    ],
                  )
                : Text(display, style: QCueText.body.copyWith(color: color)),
          ),
        ),
      ),
    );
  }
}
