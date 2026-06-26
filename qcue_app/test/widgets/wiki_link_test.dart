import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/widgets/wiki_link.dart';

void main() {
  testWidgets('S4-R14/R34: link is accent body text, tappable, routes', (
    tester,
  ) async {
    String? routed;
    await tester.pumpWidget(
      MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: Scaffold(
          body: WikiLink(
            slug: 'auto-dream',
            display: 'Auto-Dream',
            onTap: (s) => routed = s,
          ),
        ),
      ),
    );
    final text = tester.widget<Text>(find.text('Auto-Dream'));
    // S4: link TEXT uses the readable `linkText` token (>=4.5:1), not `accent`
    // (which is reserved for CTA fills). See theme_contrast_test.
    expect(
        text.style!.color, qThemeColors(QThemeId.cleanLight)[QToken.linkText]);
    expect(text.style!.fontFamily, 'Inter'); // body font, NOT mono
    final hit = tester.getSize(find.byType(WikiLink));
    expect(hit.height, greaterThanOrEqualTo(44)); // >=44pt
    await tester.tap(find.byType(WikiLink));
    expect(routed, 'auto-dream');
  });

  testWidgets('S4-R34: a dead link shows the dead-link affordance, no crash', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: Scaffold(
          body: WikiLink(
            slug: 'missing',
            display: 'Missing',
            isDead: true,
            onTap: (_) {},
          ),
        ),
      ),
    );
    expect(find.byKey(const ValueKey('dead-link')), findsOneWidget);
  });
}
