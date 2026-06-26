// QCue S4-R50/R52: read-only, pre-aggregated cost table with tabular figures.
// Cost comes STRAIGHT from cost_ledger.cost_micros (server-aggregated); the UI
// never sums messages.usage (RKM §9 #19). The month total → cap footer goes
// `pending` as it approaches the ceiling (D17).
import 'package:flutter/material.dart';
import '../../../core/models/protocol_models.dart';
import '../../../core/theme/qcue_space.dart';
import '../../../core/theme/qcue_text.dart';
import '../../../core/theme/qcue_theme.dart';

const _months = [
  'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', //
  'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec',
];

class CostLedgerTable extends StatelessWidget {
  const CostLedgerTable({
    super.key,
    required this.rows,
    required this.monthTotalMicros,
    required this.capMicros,
  });

  final List<CostLedgerRow> rows;
  final int monthTotalMicros;
  final int capMicros;

  String _usd(int micros) => '\$${(micros / 1e6).toStringAsFixed(2)}';
  String _date(DateTime d) => '${_months[d.month - 1]} ${d.day}';

  @override
  Widget build(BuildContext context) {
    final nearCap = capMicros > 0 && monthTotalMicros >= capMicros * 9 ~/ 10;
    return Semantics(
      label: 'cost ledger table',
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const _HeaderRow(),
          for (final r in rows)
            _DataRow(
              day: _date(r.day),
              inTok: '${r.inputTokens}',
              outTok: '${r.outputTokens}',
              cost: _usd(r.costMicros),
            ),
          Divider(height: 1, color: context.q.border),
          Padding(
            padding: const EdgeInsets.symmetric(
                horizontal: QSpace.md, vertical: QSpace.sm),
            child: Row(
              children: [
                Expanded(
                  child: Text('This month',
                      style: QCueText.label.copyWith(color: context.q.text)),
                ),
                Text('${_usd(monthTotalMicros)} / cap ${_usd(capMicros)}',
                    style: QCueText.monoTabular.copyWith(
                        color: nearCap ? context.q.pending : context.q.text)),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _HeaderRow extends StatelessWidget {
  const _HeaderRow();
  @override
  Widget build(BuildContext context) {
    final style = QCueText.caption.copyWith(color: context.q.text2);
    return Padding(
      padding: const EdgeInsets.symmetric(
          horizontal: QSpace.md, vertical: QSpace.xs),
      child: Row(
        children: [
          Expanded(flex: 3, child: Text('Day', style: style)),
          Expanded(
              flex: 2,
              child:
                  Text('In tok', textAlign: TextAlign.right, style: style)),
          Expanded(
              flex: 2,
              child:
                  Text('Out tok', textAlign: TextAlign.right, style: style)),
          Expanded(
              flex: 2,
              child: Text('Cost', textAlign: TextAlign.right, style: style)),
        ],
      ),
    );
  }
}

class _DataRow extends StatelessWidget {
  const _DataRow({
    required this.day,
    required this.inTok,
    required this.outTok,
    required this.cost,
  });
  final String day;
  final String inTok;
  final String outTok;
  final String cost;

  @override
  Widget build(BuildContext context) {
    final num = QCueText.monoTabular.copyWith(color: context.q.text);
    return Padding(
      padding: const EdgeInsets.symmetric(
          horizontal: QSpace.md, vertical: QSpace.xs),
      child: Row(
        children: [
          Expanded(
              flex: 3,
              child: Text(day,
                  style: QCueText.body.copyWith(color: context.q.text))),
          Expanded(
              flex: 2,
              child: Text(inTok, textAlign: TextAlign.right, style: num)),
          Expanded(
              flex: 2,
              child: Text(outTok, textAlign: TextAlign.right, style: num)),
          Expanded(
              flex: 2,
              child: Text(cost, textAlign: TextAlign.right, style: num)),
        ],
      ),
    );
  }
}
