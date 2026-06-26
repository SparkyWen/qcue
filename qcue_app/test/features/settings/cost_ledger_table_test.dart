// QCue S4-R50/R52: the cost ledger table — read-only, pre-aggregated (cost comes
// straight from cost_ledger.cost_micros; the UI never sums messages.usage), with
// tabular figures so columns align.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/settings/widgets/cost_ledger_table.dart';

void main() {
  testWidgets('S4-R50: renders pre-aggregated rows with tabular figures',
      (tester) async {
    final rows = [
      CostLedgerRow(
          day: DateTime(2026, 6, 13),
          inputTokens: 12400,
          outputTokens: 3210,
          cacheReadTokens: 0,
          cacheWriteTokens: 0,
          reasoningTokens: 0,
          costMicros: 420000),
      CostLedgerRow(
          day: DateTime(2026, 6, 12),
          inputTokens: 31002,
          outputTokens: 8114,
          cacheReadTokens: 0,
          cacheWriteTokens: 0,
          reasoningTokens: 0,
          costMicros: 1070000),
    ];
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(
        body: CostLedgerTable(
            rows: rows,
            monthTotalMicros: 14920000,
            capMicros: 150000000),
      ),
    ));
    expect(find.text(r'$0.42'), findsOneWidget); // straight from cost_micros
    final costCell = tester.widget<Text>(find.text(r'$0.42'));
    expect(costCell.style!.fontFeatures,
        contains(const FontFeature.tabularFigures()));
    expect(find.textContaining(r'$14.92'), findsOneWidget); // month total
    expect(find.textContaining(r'$150.00'), findsOneWidget); // cap
  });

  testWidgets('S4-R50: token columns use tabular figures too', (tester) async {
    final rows = [
      CostLedgerRow(
          day: DateTime(2026, 6, 13),
          inputTokens: 12400,
          outputTokens: 3210,
          cacheReadTokens: 0,
          cacheWriteTokens: 0,
          reasoningTokens: 0,
          costMicros: 420000),
    ];
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(
        body: CostLedgerTable(
            rows: rows, monthTotalMicros: 420000, capMicros: 150000000),
      ),
    ));
    final inTok = tester.widget<Text>(find.text('12400'));
    expect(inTok.style!.fontFeatures,
        contains(const FontFeature.tabularFigures()));
  });

  testWidgets('S4-R50: near-cap spend uses the pending token', (tester) async {
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: const Scaffold(
        body: CostLedgerTable(
            rows: [],
            monthTotalMicros: 145000000, // 96.7% of cap → near
            capMicros: 150000000),
      ),
    ));
    final pending = qThemeColors(QThemeId.cleanLight)[QToken.pending];
    final totalCell = tester.widget<Text>(
        find.textContaining(r'$145.00'));
    expect(totalCell.style!.color, pending);
  });
}
