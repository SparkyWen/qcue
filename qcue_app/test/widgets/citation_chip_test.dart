import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/widgets/citation_chip.dart';

void main() {
  testWidgets('S4-R39: chip is mono + info token, tappable, labelled', (
    tester,
  ) async {
    Citation? tapped;
    await tester.pumpWidget(
      MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: Scaffold(
          body: CitationChip(
            citation:
                const Citation(relPath: 'source.md', startLine: 42, endLine: 42),
            onTap: (c) => tapped = c,
          ),
        ),
      ),
    );
    final text = tester.widget<Text>(find.text('source.md:42'));
    expect(text.style!.fontFamily, 'JetBrainsMono');
    expect(text.style!.color, qThemeColors(QThemeId.cleanLight)[QToken.info]);
    await tester.tap(find.byType(CitationChip));
    expect(tapped!.relPath, 'source.md');
  });
}
