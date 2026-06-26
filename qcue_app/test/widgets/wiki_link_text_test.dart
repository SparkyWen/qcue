// QCue S4-R34/R57: WikiLinkText renders a paragraph that may contain inline
// [[wikilinks]]. Plain runs use body text; each [[link]] becomes a tappable
// `linkText`-colored span that routes to the target slug (slugified from the
// display, or an explicit `[[slug|Display]]`).
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/widgets/wiki_link_text.dart';

Widget _host(Widget child) => MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(body: child),
    );

void main() {
  testWidgets('renders plain text with no links as a single run', (tester) async {
    await tester.pumpWidget(_host(
      const WikiLinkText('just plain prose here'),
    ));
    expect(find.textContaining('just plain prose here'), findsOneWidget);
  });

  testWidgets('a [[Display]] link is linkText-colored and routes to its slug',
      (tester) async {
    String? routed;
    await tester.pumpWidget(_host(
      WikiLinkText(
        'See [[Recall Architecture]] for more.',
        onTapLink: (slug) => routed = slug,
      ),
    ));
    final richText = tester.widget<RichText>(find.byType(RichText).first);
    final link = qThemeColors(QThemeId.cleanLight)[QToken.linkText];
    // find the link span and tap its recognizer
    TextSpan? linkSpan;
    void visit(InlineSpan s) {
      if (s is TextSpan) {
        if (s.text == 'Recall Architecture' && s.style?.color == link) {
          linkSpan = s;
        }
        for (final c in s.children ?? const <InlineSpan>[]) {
          visit(c);
        }
      }
    }

    visit(richText.text);
    expect(linkSpan, isNotNull, reason: 'link span not found / not linkText');
    (linkSpan!.recognizer as TapGestureRecognizer).onTap!();
    expect(routed, 'recall-architecture'); // slugified display
  });

  testWidgets('explicit [[slug|Display]] routes to the slug, shows Display',
      (tester) async {
    String? routed;
    await tester.pumpWidget(_host(
      WikiLinkText(
        'Go to [[auto-dream|Auto-Dream]].',
        onTapLink: (slug) => routed = slug,
      ),
    ));
    final richText = tester.widget<RichText>(find.byType(RichText).first);
    TextSpan? linkSpan;
    void visit(InlineSpan s) {
      if (s is TextSpan) {
        if (s.text == 'Auto-Dream' && s.recognizer != null) linkSpan = s;
        for (final c in s.children ?? const <InlineSpan>[]) {
          visit(c);
        }
      }
    }

    visit(richText.text);
    expect(linkSpan, isNotNull);
    (linkSpan!.recognizer as TapGestureRecognizer).onTap!();
    expect(routed, 'auto-dream');
  });
}
