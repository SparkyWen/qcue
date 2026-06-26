import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';

void main() {
  testWidgets('context.q resolves the active theme tokens', (tester) async {
    late QCueColors seen;
    await tester.pumpWidget(
      MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: Builder(
          builder: (c) {
            seen = c.q;
            return const SizedBox.shrink();
          },
        ),
      ),
    );
    expect(
      seen.color(QToken.accent),
      qThemeColors(QThemeId.cleanLight)[QToken.accent],
    );
    expect(seen.color(QToken.bg), const Color(0xFFFFFFFF));
  });

  test('lerp returns a valid extension (no crash mid-animation)', () {
    final a = QCueColors.of(QThemeId.cleanLight);
    final b = QCueColors.of(QThemeId.night);
    final mid = a.lerp(b, 0.5);
    expect(mid, isA<QCueColors>());
  });
}
