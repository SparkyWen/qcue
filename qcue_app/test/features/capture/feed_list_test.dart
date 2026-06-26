// QCue S4-R33: a failed capture row exposes a ≥44pt Retry affordance that
// re-submits it; non-failed rows expose no retry.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/capture/widgets/feed_list.dart';

Idea _idea(String id, IngestState state) => Idea(
      id: id,
      tenantId: 't',
      userId: 'u',
      kind: IdeaKind.text,
      body: 'idea $id',
      origin: 'capture',
      ingestState: state,
      capturedAt: DateTime(2026, 6, 15, 12),
    );

Widget _host(Widget child) => MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(body: child),
    );

void main() {
  testWidgets('a failed row shows a >=44pt Retry that re-submits (S4-R33)',
      (tester) async {
    Idea? retried;
    await tester.pumpWidget(_host(FeedList(
      ideas: [_idea('f1', IngestState.failed)],
      now: DateTime(2026, 6, 15, 13),
      onRetry: (i) => retried = i,
    )));
    final retry = find.byKey(const ValueKey('retry-f1'));
    expect(retry, findsOneWidget);
    expect(tester.getSize(retry).height, greaterThanOrEqualTo(44));
    await tester.tap(retry);
    expect(retried?.id, 'f1');
  });

  testWidgets('non-failed rows expose no retry', (tester) async {
    await tester.pumpWidget(_host(FeedList(
      ideas: [
        _idea('a', IngestState.ingested),
        _idea('b', IngestState.pending),
      ],
      now: DateTime(2026, 6, 15, 13),
      onRetry: (_) {},
    )));
    expect(find.textContaining('Retry'), findsNothing);
  });

  // Date labels must follow the USER's local calendar day, not the stored UTC day. capturedAt is
  // persisted in UTC (multi-device sync uses unified UTC), but the feed renders in the user's zone.
  // Run under a non-UTC zone to exercise (the bug is invisible on a UTC machine): the verification
  // runs `TZ=Australia/Sydney flutter test`. Guarded to skip on a UTC host so it never false-passes.
  testWidgets('labels a UTC-stored capture by the user local day (not the UTC day)',
      (tester) async {
    // Under UTC+10 (Sydney): 2026-06-15 21:00 UTC == 2026-06-16 07:00 LOCAL — i.e. *today* locally.
    final capturedUtc = DateTime.utc(2026, 6, 15, 21, 0);
    final idea = Idea(
      id: 'tz',
      tenantId: 't',
      userId: 'u',
      kind: IdeaKind.text,
      body: 'a late-evening idea',
      origin: 'capture',
      ingestState: IngestState.ingested,
      capturedAt: capturedUtc,
    );
    // "now" is later the SAME local day: 2026-06-16 09:00 LOCAL == 2026-06-15 23:00 UTC.
    final now = DateTime.utc(2026, 6, 15, 23, 0).toLocal();
    await tester.pumpWidget(_host(FeedList(ideas: [idea], now: now)));
    expect(find.text('Today'), findsOneWidget,
        reason: 'the capture is on the user local calendar day');
    expect(find.text('Yesterday'), findsNothing);
  }, skip: DateTime.now().timeZoneOffset == Duration.zero);
}
