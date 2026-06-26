import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_space.dart';

void main() {
  test('S4-R16: 4/8 spacing scale + section rhythm + radius scale', () {
    expect(QSpace.xs, 4);
    expect(QSpace.sm, 8);
    expect(QSpace.md, 16);
    expect(QSpace.lg, 24);
    expect(QSpace.xl, 32);
    expect(QSpace.xxl, 48);
    expect(QRadius.input, 8);
    expect(QRadius.control, 10);
    expect(QRadius.card, 14);
  });

  testWidgets('S4-R17: QTarget guarantees a >=44pt hit rect', (tester) async {
    var tapped = false;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Center(
            child: QTarget(
              onTap: () => tapped = true,
              child: const Icon(Icons.add, size: 16),
            ),
          ),
        ),
      ),
    );
    final size = tester.getSize(find.byType(QTarget));
    expect(size.width, greaterThanOrEqualTo(44));
    expect(size.height, greaterThanOrEqualTo(44));
    await tester.tap(find.byType(QTarget));
    expect(tapped, isTrue);
  });
}
