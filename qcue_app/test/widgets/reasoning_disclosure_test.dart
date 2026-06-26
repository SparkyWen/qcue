// QCue S4-R38 / D18: the reasoning disclosure is collapsed by default. The
// reasoning text is hidden until the user expands it; expanding reveals it.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/widgets/reasoning_disclosure.dart';

Widget _host(Widget child) => MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(body: child),
    );

void main() {
  testWidgets('collapsed by default — reasoning hidden, toggle visible',
      (tester) async {
    await tester.pumpWidget(_host(
      const ReasoningDisclosure(reasoning: 'chain of thought here'),
    ));
    expect(find.text('chain of thought here'), findsNothing);
    expect(find.text('Reasoning'), findsOneWidget); // the summary toggle
  });

  testWidgets('tapping the toggle reveals the reasoning', (tester) async {
    await tester.pumpWidget(_host(
      const ReasoningDisclosure(reasoning: 'chain of thought here'),
    ));
    await tester.tap(find.text('Reasoning'));
    await tester.pumpAndSettle();
    expect(find.text('chain of thought here'), findsOneWidget);
  });

  testWidgets('renders nothing when there is no reasoning', (tester) async {
    await tester.pumpWidget(_host(const ReasoningDisclosure(reasoning: '')));
    expect(find.text('Reasoning'), findsNothing);
  });
}
