// QCue S4-R13: WCAG 2.1 relative-luminance + contrast-ratio math. Pure (no
// Flutter binding) so the theme-contrast gate runs as a plain unit test.
import 'dart:math' as math;
import 'dart:ui';

double _linear(double channel8bit) {
  final s = channel8bit / 255.0;
  return s <= 0.03928
      ? s / 12.92
      : math.pow((s + 0.055) / 1.055, 2.4).toDouble();
}

double _relativeLuminance(Color c) =>
    0.2126 * _linear((c.r * 255.0).roundToDouble()) +
    0.7152 * _linear((c.g * 255.0).roundToDouble()) +
    0.0722 * _linear((c.b * 255.0).roundToDouble());

/// WCAG 2.1 contrast ratio in `[1, 21]`. Order-independent.
double contrastRatio(Color a, Color b) {
  final la = _relativeLuminance(a);
  final lb = _relativeLuminance(b);
  final hi = math.max(la, lb);
  final lo = math.min(la, lb);
  return (hi + 0.05) / (lo + 0.05);
}
