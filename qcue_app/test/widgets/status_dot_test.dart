// QCue S4-R30/R32: the ingest-status dot — a small colored dot whose color
// maps from `ingest_state`, with an accessible label.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/theme/qcue_motion.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/widgets/status_dot.dart';

void main() {
  Color dotColor(WidgetTester tester) {
    final box = tester.widget<DecoratedBox>(
      find.descendant(
        of: find.byType(StatusDot),
        matching: find.byType(DecoratedBox),
      ),
    );
    return (box.decoration as BoxDecoration).color!;
  }

  Future<void> pump(WidgetTester tester, IngestState state) async {
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(body: Center(child: StatusDot(state: state))),
    ));
  }

  final colors = qThemeColors(QThemeId.cleanLight);

  testWidgets('ingested → success', (tester) async {
    await pump(tester, IngestState.ingested);
    expect(dotColor(tester), colors[QToken.success]);
  });

  testWidgets('pending → pending token', (tester) async {
    await pump(tester, IngestState.pending);
    expect(dotColor(tester), colors[QToken.pending]);
  });

  testWidgets('ingesting → info', (tester) async {
    await pump(tester, IngestState.ingesting);
    expect(dotColor(tester), colors[QToken.info]);
  });

  testWidgets('skippedRedundant → text3 (muted)', (tester) async {
    await pump(tester, IngestState.skippedRedundant);
    expect(dotColor(tester), colors[QToken.text3]);
  });

  testWidgets('failed → danger', (tester) async {
    await pump(tester, IngestState.failed);
    expect(dotColor(tester), colors[QToken.danger]);
  });

  testWidgets('carries an accessible label', (tester) async {
    await pump(tester, IngestState.failed);
    expect(
      find.bySemanticsLabel(RegExp('failed', caseSensitive: false)),
      findsOneWidget,
    );
  });

  testWidgets('queued capture is a distinct hollow dot, pending hue',
      (tester) async {
    // A locally-queued (offline) capture reuses the pending hue but renders a
    // distinct hollow/outlined dot so it is visibly different from a server
    // pending row — and carries a "queued, will sync" label.
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: const Scaffold(
        body: Center(child: StatusDot(state: IngestState.pending, queued: true)),
      ),
    ));
    final box = tester.widget<DecoratedBox>(
      find.descendant(
        of: find.byType(StatusDot),
        matching: find.byType(DecoratedBox),
      ),
    );
    final dec = box.decoration as BoxDecoration;
    // hollow: no fill, a pending-colored ring
    expect(dec.color, isNull);
    expect(dec.border, isNotNull);
    expect(
      find.bySemanticsLabel(RegExp('queued', caseSensitive: false)),
      findsOneWidget,
    );
  });

  testWidgets('S4-R32: dot animates in place, instant under reduced motion',
      (tester) async {
    AnimatedContainer animated(WidgetTester t) => t.widget<AnimatedContainer>(
          find.descendant(
            of: find.byType(StatusDot),
            matching: find.byType(AnimatedContainer),
          ),
        );

    // Normal: a finite 150-300ms color animation (so it settles — test-safe).
    await pump(tester, IngestState.pending);
    expect(animated(tester).duration, QMotion.base);
    expect(animated(tester).duration.inMilliseconds, inInclusiveRange(150, 300));

    // Reduced motion: collapsed to instant (S4-R61).
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Builder(
        builder: (context) => MediaQuery(
          data: MediaQuery.of(context).copyWith(disableAnimations: true),
          child: const Scaffold(
            body: Center(child: StatusDot(state: IngestState.pending)),
          ),
        ),
      ),
    ));
    expect(animated(tester).duration, Duration.zero);
  });
}
