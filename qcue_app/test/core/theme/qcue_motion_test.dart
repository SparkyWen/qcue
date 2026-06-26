import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_motion.dart';

void main() {
  test('S4-R18: durations fall in the 150-300ms band', () {
    expect(QMotion.fast.inMilliseconds, 150);
    expect(QMotion.base.inMilliseconds, 220);
    expect(QMotion.slow.inMilliseconds, 300);
    expect(QMotion.fast.inMilliseconds, greaterThanOrEqualTo(150));
    expect(QMotion.slow.inMilliseconds, lessThanOrEqualTo(300));
  });

  testWidgets('S4-R18: reducedMotion is true when OS disables animations', (
    tester,
  ) async {
    late bool reduced;
    await tester.pumpWidget(
      MediaQuery(
        data: const MediaQueryData(disableAnimations: true),
        child: Builder(
          builder: (c) {
            reduced = QMotion.reduced(c);
            return const SizedBox.shrink();
          },
        ),
      ),
    );
    expect(reduced, isTrue);
  });

  testWidgets('S4-R18: durationOrZero collapses to zero under reduced motion', (
    tester,
  ) async {
    late Duration d;
    await tester.pumpWidget(
      MediaQuery(
        data: const MediaQueryData(disableAnimations: true),
        child: Builder(
          builder: (c) {
            d = QMotion.durationOrZero(c, QMotion.base);
            return const SizedBox.shrink();
          },
        ),
      ),
    );
    expect(d, Duration.zero);
  });
}
