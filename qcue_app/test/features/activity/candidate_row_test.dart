// QCue S4-R41 / D13: a candidate row shows the proposed change (target slug +
// summary) with Approve/Reject. Destructive deletes get a confirmation dialog
// before the decision fires; the merge Approve fires directly (already gated by
// being a candidate). Disabled while offline.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/activity/widgets/candidate_row.dart';

Approval _merge() => const Approval(
      id: 'ap-1',
      action: 'wiki_merge',
      status: ApprovalStatus.pending,
      requestedBy: 'dream',
      subjectRef: {
        'target_slug': 'recall-architecture',
        'summary': 'Merge Grep recall into Recall Architecture.',
      },
    );

Approval _delete() => const Approval(
      id: 'ap-2',
      action: 'wiki_delete',
      status: ApprovalStatus.pending,
      requestedBy: 'dream',
      subjectRef: {
        'target_slug': 'stale-note',
        'summary': 'Delete the orphaned Stale note page.',
      },
    );

Widget _host(Widget child) => MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(body: child),
    );

void main() {
  testWidgets('S4-R41: shows the proposed change (slug + summary) + actions',
      (tester) async {
    await tester.pumpWidget(_host(
      CandidateRow(approval: _merge(), onDecide: (_) {}, disabled: false),
    ));
    expect(find.textContaining('Merge Grep recall'), findsOneWidget);
    expect(find.textContaining('recall-architecture'), findsOneWidget);
    expect(find.text('Approve'), findsOneWidget);
    expect(find.text('Reject'), findsOneWidget);
  });

  testWidgets('S4-R41: a non-destructive merge Approve fires immediately',
      (tester) async {
    bool? decided;
    await tester.pumpWidget(_host(
      CandidateRow(
          approval: _merge(),
          onDecide: (approve) => decided = approve,
          disabled: false),
    ));
    await tester.tap(find.text('Approve'));
    await tester.pumpAndSettle();
    expect(decided, isTrue); // no dialog needed for a merge
  });

  testWidgets('D13: a destructive delete Approve asks to confirm first',
      (tester) async {
    bool? decided;
    await tester.pumpWidget(_host(
      CandidateRow(
          approval: _delete(),
          onDecide: (approve) => decided = approve,
          disabled: false),
    ));
    await tester.tap(find.text('Approve'));
    await tester.pumpAndSettle();
    // a confirmation dialog appears; the decision has NOT fired yet
    expect(decided, isNull);
    expect(find.byType(AlertDialog), findsOneWidget);
    // confirming in the dialog finally fires the destructive decision
    await tester.tap(find.widgetWithText(TextButton, 'Delete'));
    await tester.pumpAndSettle();
    expect(decided, isTrue);
  });

  testWidgets('D13: dismissing the delete confirm does NOT decide',
      (tester) async {
    bool? decided;
    await tester.pumpWidget(_host(
      CandidateRow(
          approval: _delete(),
          onDecide: (approve) => decided = approve,
          disabled: false),
    ));
    await tester.tap(find.text('Approve'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, 'Cancel'));
    await tester.pumpAndSettle();
    expect(decided, isNull);
  });

  testWidgets('S4-R44: disabled (offline) → actions are inert', (tester) async {
    var fired = false;
    await tester.pumpWidget(_host(
      CandidateRow(
          approval: _merge(),
          onDecide: (_) => fired = true,
          disabled: true),
    ));
    final approve = tester.widget<FilledButton>(
        find.widgetWithText(FilledButton, 'Approve'));
    expect(approve.onPressed, isNull);
    expect(fired, isFalse);
  });

  testWidgets('S4-R41: destructive actions use the danger token', (tester) async {
    await tester.pumpWidget(_host(
      CandidateRow(approval: _delete(), onDecide: (_) {}, disabled: false),
    ));
    final danger = qThemeColors(QThemeId.cleanLight)[QToken.danger];
    final reject = tester
        .widget<TextButton>(find.widgetWithText(TextButton, 'Reject'));
    final style = reject.style?.foregroundColor
        ?.resolve(<WidgetState>{});
    expect(style, danger);
  });
}
