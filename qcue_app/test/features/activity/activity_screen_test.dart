// QCue S4: the Activity screen body wires the three sections through the single
// apiClientProvider seam: (1) ingest-review candidates (Approve/Reject → the D13
// confirm gate, the row drops on decide), (2) the live Dream card + its
// completed "Improved N pages" entry, (3) recent jobs mapped to glyphs + today's
// cost. Empty review state reads "Nothing to review."
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/theme/qcue_haptics.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/core/theme/theme_provider.dart';
import 'package:qcue_app/features/activity/activity_screen.dart';
import 'package:qcue_app/features/activity/widgets/candidate_row.dart';
import 'package:qcue_app/features/activity/widgets/job_row.dart';

class SpyHaptics implements HapticsSink {
  int light = 0, selection = 0, ok = 0;
  @override
  void lightImpact() => light++;
  @override
  void selectionClick() => selection++;
  @override
  void success() => ok++;
}

Widget _app(StubApiClient api, {SpyHaptics? spy}) => ProviderScope(
      overrides: [
        apiClientProvider.overrideWithValue(api),
        offlineProvider.overrideWith((ref) => false),
        if (spy != null) hapticsSinkProvider.overrideWithValue(spy),
      ],
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const Scaffold(body: ActivityScreen()),
      ),
    );

void main() {
  testWidgets('renders the three sections from the seeded seam', (tester) async {
    await tester.pumpWidget(_app(StubApiClient.seeded()));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('section-Review')), findsOneWidget);
    expect(find.byKey(const ValueKey('section-Dream')), findsOneWidget);
    expect(find.byKey(const ValueKey('section-Jobs')), findsOneWidget);
    // candidate rows + job rows from the fixtures
    expect(find.byType(CandidateRow), findsWidgets);
    expect(find.byType(JobRowView), findsWidgets);
    // today's cost figure
    expect(find.textContaining(r'$'), findsWidgets);
  });

  testWidgets('S4-R41/D13: approving a candidate removes its row', (tester) async {
    await tester.pumpWidget(_app(StubApiClient.seeded()));
    await tester.pumpAndSettle();
    final before = tester.widgetList<CandidateRow>(find.byType(CandidateRow)).length;
    expect(before, greaterThan(0));
    // the merge candidate (non-destructive) approves directly
    final mergeApprove = find.descendant(
      of: find.byWidgetPredicate(
          (w) => w is CandidateRow && w.approval.action == 'wiki_merge'),
      matching: find.text('Approve'),
    );
    await tester.tap(mergeApprove);
    await tester.pumpAndSettle();
    final after = tester.widgetList<CandidateRow>(find.byType(CandidateRow)).length;
    expect(after, before - 1); // the row dropped (resolved via respondApproval)
  });

  testWidgets('S4-R54: approving a candidate fires the confirm haptic',
      (tester) async {
    final spy = SpyHaptics();
    await tester.pumpWidget(_app(StubApiClient.seeded(), spy: spy));
    await tester.pumpAndSettle();
    final mergeApprove = find.descendant(
      of: find.byWidgetPredicate(
          (w) => w is CandidateRow && w.approval.action == 'wiki_merge'),
      matching: find.text('Approve'),
    );
    await tester.tap(mergeApprove);
    await tester.pump();
    await tester.pump(); // let respondApproval resolve so confirmed() fires
    expect(spy.selection, 1, reason: 'confirmed() → selectionClick on approve');
    // the seeded done dream was primed on mount → no success buzz fired.
    expect(spy.ok, 0, reason: 'priming suppresses the mount-time dream haptic');
  });

  testWidgets('empty review shows "Nothing to review."', (tester) async {
    final api = StubApiClient.seeded();
    // resolve all pending candidates
    final pending = await api.approvals();
    for (final a in pending) {
      await api.respondApproval(a.id, false);
    }
    await tester.pumpWidget(_app(api));
    await tester.pumpAndSettle();
    expect(find.text('Nothing to review.'), findsOneWidget);
  });
}
