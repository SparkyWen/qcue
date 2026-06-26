// QCue S4-R43 / A-R38: the Dream-completion feed entry — "Improved N pages",
// mirroring Claude's inline "Improved N files" (verb 'Improved'). Taps through
// to the candidates/diff. When the Dream produced confirm-required merges/
// deletes, a needs-review badge (pending token) shows.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/activity/widgets/improved_pages_entry.dart';

void main() {
  testWidgets('S4-R43: shows "Improved N pages", needs-review badge, taps through',
      (tester) async {
    var tapped = false;
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(
        body: ImprovedPagesEntry(
          pagesImproved: 7,
          needsReview: true,
          onTap: () => tapped = true,
        ),
      ),
    ));
    expect(find.text('Improved 7 pages'), findsOneWidget);
    expect(find.byKey(const ValueKey('needs-review-badge')), findsOneWidget);
    await tester.tap(find.byType(ImprovedPagesEntry));
    expect(tapped, isTrue); // taps through to candidates/diff
  });

  testWidgets('S4-R43: no badge when there is nothing to review', (tester) async {
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(
        body: ImprovedPagesEntry(
            pagesImproved: 3, needsReview: false, onTap: () {}),
      ),
    ));
    expect(find.byKey(const ValueKey('needs-review-badge')), findsNothing);
    expect(find.text('Improved 3 pages'), findsOneWidget);
  });

  testWidgets('S4-R43: singular page reads "Improved 1 page"', (tester) async {
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(
        body: ImprovedPagesEntry(
            pagesImproved: 1, needsReview: false, onTap: () {}),
      ),
    ));
    expect(find.text('Improved 1 page'), findsOneWidget);
  });
}
