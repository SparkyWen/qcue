// QCue S4-R18: motion is 150-300ms with gentle easing; a single reduced-motion
// gate disables/shortens it (master §10). transform/opacity only.
import 'package:flutter/material.dart';

abstract final class QMotion {
  static const fast = Duration(milliseconds: 150);
  static const base = Duration(milliseconds: 220);
  static const slow = Duration(milliseconds: 300);
  static const enter = Curves.easeOutCubic;
  static const exit = Curves.easeInCubic;

  /// True when the OS requests reduced motion.
  static bool reduced(BuildContext context) =>
      MediaQuery.maybeDisableAnimationsOf(context) ?? false;

  /// The duration to use, collapsed to zero under reduced motion so content is
  /// never gated by animation.
  static Duration durationOrZero(BuildContext context, Duration d) =>
      reduced(context) ? Duration.zero : d;
}
