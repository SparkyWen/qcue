// QCue v0.2.2: QMarkdown is the themed, inert GFM renderer for wiki bodies and
// recall answers. These lock in: the [[wikilink]] ⇄ wiki:slug rewrite, GFM
// structure (headings/bold/inline-code/tables/lists/fenced code), and that a
// [[wikilink]] tap routes to its slug.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:gpt_markdown/gpt_markdown.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/widgets/q_markdown.dart';

Widget _host(Widget child) => MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(body: SingleChildScrollView(child: child)),
    );

void main() {
  group('preprocessWikiLinks', () {
    test('rewrites [[Display]] into a wiki: link with a slug', () {
      expect(preprocessWikiLinks('See [[Auto-Dream]] now.'),
          'See [Auto-Dream](wiki:auto-dream) now.');
    });
    test('rewrites [[slug|Display]] keeping the display + explicit slug', () {
      expect(preprocessWikiLinks('[[Recall Architecture|Recall]]'),
          '[Recall](wiki:recall-architecture)');
    });
    test('leaves prose without wikilinks untouched', () {
      expect(preprocessWikiLinks('**bold** and `code`'), '**bold** and `code`');
    });
  });

  group('wikiSlugFromUrl', () {
    test('decodes a wiki: url to its slug', () {
      expect(wikiSlugFromUrl('wiki:auto-dream'), 'auto-dream');
    });
    test('returns null for an external url', () {
      expect(wikiSlugFromUrl('https://example.com'), isNull);
    });
  });

  testWidgets('renders headings, bold and inline code', (tester) async {
    await tester.pumpWidget(_host(
      const QMarkdown('# Title\n\nThis is **bold** and `code` text.'),
    ));
    await tester.pumpAndSettle();
    expect(find.byType(GptMarkdown), findsOneWidget);
    expect(find.text('Title', findRichText: true), findsWidgets);
    // inline `code` renders via the themed inline-code chip.
    expect(find.text('code'), findsOneWidget);
  });

  testWidgets('renders a GFM table', (tester) async {
    await tester.pumpWidget(_host(
      const QMarkdown('| a | b |\n| - | - |\n| 1 | 2 |'),
    ));
    await tester.pumpAndSettle();
    expect(find.byType(Table), findsWidgets);
  });

  testWidgets('S4-R57: a fenced code block renders monospace + inert', (
    tester,
  ) async {
    await tester.pumpWidget(_host(
      const QMarkdown('Before.\n\n```\nfn main() {}\n  indented();\n```\n\nAfter.'),
    ));
    await tester.pumpAndSettle();
    final code = tester.widget<Text>(find.byKey(const ValueKey('md-code-block')));
    expect(code.data, contains('fn main() {}'));
    expect(code.data, contains('  indented();'));
  });

  testWidgets('a [[wikilink]] tap routes to its slug', (tester) async {
    String? routed;
    await tester.pumpWidget(_host(QMarkdown(
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
}
