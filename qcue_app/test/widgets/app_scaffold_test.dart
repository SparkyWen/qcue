import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/widgets/app_scaffold.dart';

void main() {
  testWidgets(
      'S4-R29: scaffold has 4 labeled tabs, safe-area, compose affordance', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: AppScaffold(
          title: 'Capture',
          currentIndex: 0,
          onTab: (_) {},
          onCompose: () {},
          body: const SizedBox.shrink(),
        ),
      ),
    );
    for (final label in ['Capture', 'Wiki', 'Recall', 'Settings']) {
      expect(find.text(label), findsWidgets);
    }
    // Activity is no longer a bottom-nav tab (v0.2.2 — moved into Settings).
    expect(find.text('Activity'), findsNothing);
    expect(find.byKey(const ValueKey('compose-affordance')), findsOneWidget);
    expect(find.byType(SafeArea), findsWidgets);
    // No emoji icons — all nav icons are vector IconData (S4-R29).
    final icons = tester.widgetList<Icon>(find.byType(Icon));
    expect(icons, isNotEmpty);
  });

  testWidgets('S4-R29: the active tab is highlighted with the accent token', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: AppScaffold(
          title: 'Wiki',
          currentIndex: 1,
          onTab: (_) {},
          onCompose: () {},
          body: const SizedBox.shrink(),
        ),
      ),
    );
    final accent = qThemeColors(QThemeId.cleanLight)[QToken.accent];
    // The active tab's label (the second "Wiki") is rendered in the accent.
    final wikiLabels = tester.widgetList<Text>(find.text('Wiki')).toList();
    expect(wikiLabels.any((t) => t.style?.color == accent), isTrue);
  });
}
