// QCue S4-R37 (v0.2.2): StreamingText renders an answer that grows token-by-
// token as full markdown via QMarkdown, with inline [[wikilinks]] that route. A
// trailing caret shows while streaming and disappears when done.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:gpt_markdown/gpt_markdown.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/widgets/streaming_text.dart';

Widget _host(Widget child) => MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(body: SingleChildScrollView(child: child)),
    );

void main() {
  testWidgets('renders the accumulated text + a caret while streaming',
      (tester) async {
    await tester.pumpWidget(_host(
      const StreamingText(text: 'You decided **X**', streaming: true),
    ));
    await tester.pumpAndSettle();
    expect(find.byType(GptMarkdown), findsOneWidget);
    expect(find.byKey(const ValueKey('stream-caret')), findsOneWidget);
  });

  testWidgets('caret disappears when streaming completes', (tester) async {
    await tester.pumpWidget(_host(
      const StreamingText(text: 'Final answer.', streaming: false),
    ));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('stream-caret')), findsNothing);
  });

  testWidgets('inline [[wikilink]] in the answer routes', (tester) async {
    String? routed;
    await tester.pumpWidget(_host(
      StreamingText(
        text: 'See [[Recall Architecture]].',
        streaming: false,
        onTapLink: (slug) => routed = slug,
      ),
    ));
    await tester.pumpAndSettle();
    final link = find.text('Recall Architecture', findRichText: true);
    expect(link, findsWidgets);
    await tester.tap(link.first);
    await tester.pumpAndSettle();
    expect(routed, 'recall-architecture');
  });
}
