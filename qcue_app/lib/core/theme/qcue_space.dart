// QCue S4-R16/R17: 4/8 spacing + section rhythm + radius scale (master §10),
// plus a ≥44pt touch-target helper.
import 'package:flutter/material.dart';

/// 4/8 spacing; section rhythm 16/24/32/48 (master §10).
abstract final class QSpace {
  static const double xs = 4;
  static const double sm = 8;
  static const double md = 16;
  static const double lg = 24;
  static const double xl = 32;
  static const double xxl = 48;
}

/// Radius scale: inputs 8, controls 10, cards/sheets 14 (master §10).
abstract final class QRadius {
  static const double input = 8;
  static const double control = 10;
  static const double card = 14;
}

/// Wraps any (possibly small) tappable to guarantee a ≥44pt hit area
/// (master §10 touch-target-minimum) without enlarging the visible glyph.
class QTarget extends StatelessWidget {
  const QTarget({
    super.key,
    required this.child,
    required this.onTap,
    this.semanticLabel,
    this.minSize = 44,
  });

  final Widget child;
  final VoidCallback onTap;
  final String? semanticLabel;
  final double minSize;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      label: semanticLabel,
      child: InkResponse(
        onTap: onTap,
        radius: minSize / 2,
        child: ConstrainedBox(
          constraints: BoxConstraints(minWidth: minSize, minHeight: minSize),
          child: Center(child: child),
        ),
      ),
    );
  }
}
