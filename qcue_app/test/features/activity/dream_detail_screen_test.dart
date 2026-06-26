// QCue S4-R42 / A-R37: the Dream detail screen replicates DreamDetailDialog —
// title "Memory consolidation"; subtitle = elapsed · reviewing N sessions · M
// pages touched (live); status colored via tokens; the LAST 6 turns with
// tool-uses collapsed to a count, earlier ones collapsed to "(K earlier turns)"
// (VISIBLE_TURNS=6); pages-touched ("at least these"); a Cancel that maps to
// DreamTask.kill → clock rollback. Reasoning stays collapsed-by-default (D18).
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/activity/dream_detail_screen.dart';
import 'package:qcue_app/features/activity/dream_provider.dart';
import 'package:qcue_app/widgets/reasoning_disclosure.dart';

void main() {
  testWidgets('S4-R42: title, last 6 turns + collapse label, live counts, Cancel',
      (tester) async {
    final turns = [
      for (var i = 0; i < 8; i++)
        DreamTurn(
            text: 'turn $i',
            toolUseCount: i,
            pagesTouched: const ['index.md']),
    ];
    var cancelled = false;
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: DreamDetailScreen(
        state: DreamState(
          title: 'Memory consolidation',
          status: DreamStatus.running,
          sessionsReviewing: 6,
          pagesTouched: const ['index.md', 'concepts/auto-dream.md'],
          turns: turns,
          elapsed: const Duration(seconds: 48),
        ),
        onCancel: () => cancelled = true,
      ),
    ));
    expect(find.text('Memory consolidation'), findsOneWidget);
    // Only the LAST 6 turns with text are visible; earlier collapse to a label.
    expect(find.text('turn 7'), findsOneWidget);
    expect(find.text('turn 1'), findsNothing);
    expect(find.text('(2 earlier turns)'), findsOneWidget);
    expect(find.textContaining('reviewing 6 session'), findsOneWidget);
    expect(find.textContaining('(7 tool)'), findsOneWidget); // tool count
    await tester.tap(find.text('Cancel'));
    expect(cancelled, isTrue); // → DreamTask.kill / clock rollback (A-R8)
  });

  testWidgets('S4-R42 / D18: reasoning is collapsed-by-default', (tester) async {
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: DreamDetailScreen(
        state: const DreamState(
          title: 'Memory consolidation',
          status: DreamStatus.running,
          reasoning: 'the model chain-of-thought while consolidating',
        ),
        onCancel: () {},
      ),
    ));
    expect(find.byType(ReasoningDisclosure), findsOneWidget);
    expect(find.text('the model chain-of-thought while consolidating'),
        findsNothing);
  });

  testWidgets('S4-R42: a completed dream shows no Cancel control', (tester) async {
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: DreamDetailScreen(
        state: const DreamState(
          title: 'Memory consolidation',
          status: DreamStatus.completed,
          pagesImproved: 3,
        ),
        onCancel: () {},
      ),
    ));
    expect(find.text('Cancel'), findsNothing);
  });

  test('S4-R42: dream provider folds SSE progress into turns + counts', () {
    const s0 = DreamState(
        title: 'Memory consolidation', status: DreamStatus.running);
    final s1 = applyDreamEvent(s0, const DreamStarted(jobId: 'd1', sessions: 6));
    final s2 = applyDreamEvent(
        s1,
        const DreamProgress(
            text: 'Merged dupes',
            toolUseCount: 5,
            pagesTouched: ['index.md']));
    final s3 = applyDreamEvent(s2, const DreamCompleted(pagesImproved: 7));
    expect(s2.sessionsReviewing, 6);
    expect(s2.turns.single.toolUseCount, 5);
    expect(s2.pagesTouched, contains('index.md'));
    expect(s3.status, DreamStatus.completed);
    expect(s3.pagesImproved, 7);
  });

  test('S4-R42: progress dedups touched pages across turns', () {
    const s0 = DreamState(
        title: 'Memory consolidation', status: DreamStatus.running);
    final s1 = applyDreamEvent(
        s0,
        const DreamProgress(
            text: 't0', toolUseCount: 1, pagesTouched: ['index.md']));
    final s2 = applyDreamEvent(
        s1,
        const DreamProgress(
            text: 't1',
            toolUseCount: 2,
            pagesTouched: ['index.md', 'a.md']));
    expect(s2.pagesTouched.toSet(), {'index.md', 'a.md'});
    expect(s2.turns.length, 2);
  });

  test('S4-R42: a failure folds into the failed status + reason', () {
    const s0 = DreamState(
        title: 'Memory consolidation', status: DreamStatus.running);
    final s1 = applyDreamEvent(s0, const DreamFailed('provider exhausted'));
    expect(s1.status, DreamStatus.failed);
    expect(s1.error, 'provider exhausted');
  });

  test('S4-R42: reasoning deltas fold into the (collapsed) reasoning', () {
    const s0 = DreamState(
        title: 'Memory consolidation', status: DreamStatus.running);
    final s1 = applyDreamEvent(s0, const ReasoningDelta('thinking '));
    final s2 = applyDreamEvent(s1, const ReasoningDelta('hard'));
    expect(s2.reasoning, 'thinking hard');
  });
}
