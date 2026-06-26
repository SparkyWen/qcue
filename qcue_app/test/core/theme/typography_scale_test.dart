import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_text.dart';

void main() {
  test('S4-R15: named styles use the exact scale, families, line-heights', () {
    expect(QCueText.caption.fontSize, 13);
    expect(QCueText.label.fontSize, 15);
    expect(QCueText.label.fontWeight, FontWeight.w500);
    expect(QCueText.body.fontSize, 17);
    expect(QCueText.body.height, 1.6);
    expect(QCueText.body.fontWeight, FontWeight.w400);
    expect(QCueText.subtitle.fontSize, 20);
    expect(QCueText.title.fontSize, 28);
    expect(QCueText.title.fontWeight, FontWeight.w600);
    expect(QCueText.display.fontSize, 34);
    expect(QCueText.body.fontFamily, 'Inter');
    expect(QCueText.mono.fontFamily, 'JetBrainsMono');
    expect(QCueText.mono.height, 1.4);
  });

  test('S4-R15: monoTabular enables tabular figures', () {
    expect(
      QCueText.monoTabular.fontFeatures,
      contains(const FontFeature.tabularFigures()),
    );
  });
}
