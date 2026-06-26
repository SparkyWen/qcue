import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_haptics.dart';

class _SpyHaptics implements HapticsSink {
  final calls = <String>[];
  @override
  void lightImpact() => calls.add('light');
  @override
  void selectionClick() => calls.add('selection');
  @override
  void success() => calls.add('success');
}

void main() {
  test('S4-R54: only the 3 key moments fire, mapped correctly', () {
    final spy = _SpyHaptics();
    final h = Haptics(spy);
    h.captureCommitted();
    h.confirmed();
    h.dreamCompleted();
    expect(spy.calls, ['light', 'selection', 'success']);
  });
}
