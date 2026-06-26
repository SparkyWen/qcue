// QCue S4-R57/R63 (v0.2.2): MarkdownView renders a wiki body as full markdown
// via QMarkdown (gpt_markdown) — headings, bullets, paragraphs, fenced code —
// with inline [[wikilinks]] preserved and routed. Theme-styled and inert: links
// route, nothing executes.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:gpt_markdown/gpt_markdown.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/widgets/markdown_view.dart';

Widget _host(Widget child) => MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(body: SingleChildScrollView(child: child)),
    );

void main() {
  testWidgets('renders a heading, a bullet, and a paragraph', (tester) async {
    await tester.pumpWidget(_host(const MarkdownView(
      '## Title\n\nA paragraph of prose.\n\n- first bullet\n- second bullet',
    )));
    await tester.pumpAndSettle();
    expect(find.byType(GptMarkdown), findsOneWidget);
    expect(find.textContaining('Title', findRichText: true), findsWidgets);
    expect(find.textContaining('A paragraph of prose.', findRichText: true),
        findsWidgets);
    expect(find.textContaining('first bullet', findRichText: true),
        findsWidgets);
    expect(find.textContaining('second bullet', findRichText: true),
        findsWidgets);
  });

  testWidgets('inline [[wikilink]] in a paragraph routes to its slug',
      (tester) async {
    String? routed;
    await tester.pumpWidget(_host(MarkdownView(
      'See [[Auto-Dream]] for details.',
      onTapLink: (slug) => routed = slug,
    )));
    await tester.pumpAndSettle();
    final link = find.text('Auto-Dream', findRichText: true);
    expect(link, findsWidgets);
    await tester.tap(link.first);
    await tester.pumpAndSettle();
    expect(routed, 'auto-dream');
  });

  testWidgets('S4-R57: a fenced ``` code block renders monospace, verbatim',
      (tester) async {
    await tester.pumpWidget(_host(const MarkdownView(
      'Before.\n\n```\nfn main() {}\n  indented();\n```\n\nAfter.',
    )));
    await tester.pumpAndSettle();
    final code =
        tester.widget<Text>(find.byKey(const ValueKey('md-code-block')));
    // content is verbatim — indentation preserved, fences stripped.
    expect(code.data, contains('fn main() {}'));
    expect(code.data, contains('  indented();'));
    // surrounding prose still renders as normal blocks.
    expect(find.textContaining('Before.', findRichText: true), findsWidgets);
    expect(find.textContaining('After.', findRichText: true), findsWidgets);
  });
}
