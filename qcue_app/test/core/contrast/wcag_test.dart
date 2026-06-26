import 'dart:ui';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/contrast/wcag.dart';

void main() {
  test('S4-R13: black on white is 21:1', () {
    final r = contrastRatio(const Color(0xFF000000), const Color(0xFFFFFFFF));
    expect(r, closeTo(21.0, 0.01));
  });

  test('S4-R13: identical colors are 1:1', () {
    final r = contrastRatio(const Color(0xFF37352F), const Color(0xFF37352F));
    expect(r, closeTo(1.0, 0.001));
  });

  test('S4-R13: order does not matter', () {
    final a = contrastRatio(const Color(0xFF2563EB), const Color(0xFFFFFFFF));
    final b = contrastRatio(const Color(0xFFFFFFFF), const Color(0xFF2563EB));
    expect(a, closeTo(b, 0.0001));
  });
}
