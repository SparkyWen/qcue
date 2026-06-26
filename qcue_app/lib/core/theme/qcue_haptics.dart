// QCue S4-R54: haptics fire on exactly three key moments. The sink is
// injectable so tests can spy and so it no-ops where the OS disables haptics.
import 'package:flutter/services.dart';

abstract interface class HapticsSink {
  void lightImpact();
  void selectionClick();
  void success();
}

class PlatformHaptics implements HapticsSink {
  const PlatformHaptics();
  @override
  void lightImpact() => HapticFeedback.lightImpact();
  @override
  void selectionClick() => HapticFeedback.selectionClick();
  @override
  void success() => HapticFeedback.heavyImpact();
}

/// Fires haptics on exactly the three key moments (S4-R54).
class Haptics {
  const Haptics([this._sink = const PlatformHaptics()]);
  final HapticsSink _sink;

  void captureCommitted() => _sink.lightImpact(); // capture committed
  void confirmed() => _sink.selectionClick(); // approve / confirm
  void dreamCompleted() => _sink.success(); // Dream complete
}
