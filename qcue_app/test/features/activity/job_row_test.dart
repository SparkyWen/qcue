// QCue S4-R44: the job row maps every jobs.state onto a glyph + label + semantic
// token (queued/running/completed/failed/canceled/skipped), never color-only.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/activity/widgets/job_row.dart';

void main() {
  testWidgets('S4-R44: every job_state maps to a glyph + accessible label',
      (tester) async {
    for (final s in JobState.values) {
      await tester.pumpWidget(MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: Scaffold(
          body: JobRowView(
            job: JobRow(id: 'j', kind: JobKind.ingest, state: s),
          ),
        ),
      ));
      final sem = tester.widget<Semantics>(find
          .descendant(
              of: find.byType(JobRowView), matching: find.byType(Semantics))
          .first);
      expect(sem.properties.label, isNotNull);
      expect(sem.properties.label, isNotEmpty);
      // an icon glyph accompanies the color (never color-only)
      expect(
          find.descendant(
              of: find.byType(JobRowView), matching: find.byType(Icon)),
          findsOneWidget);
    }
  });
}
