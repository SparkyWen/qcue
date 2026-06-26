// QCue S4-R56: the offline banner. Shows only while offline; announces queued
// captures will sync; a retry affordance.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/widgets/offline_banner.dart';

Widget _host(Widget child) => MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(body: child),
    );

void main() {
  testWidgets('hidden when online (renders nothing)', (tester) async {
    await tester.pumpWidget(_host(const OfflineBanner(offline: false)));
    expect(find.byType(SizedBox), findsWidgets); // shrink
    expect(find.textContaining('Offline'), findsNothing);
  });

  testWidgets('shown when offline with a queued-sync message', (tester) async {
    await tester.pumpWidget(_host(const OfflineBanner(offline: true)));
    expect(find.textContaining('Offline'), findsOneWidget);
    expect(find.textContaining('sync'), findsOneWidget);
  });

  testWidgets('retry button fires onReconnect', (tester) async {
    var taps = 0;
    await tester.pumpWidget(
        _host(OfflineBanner(offline: true, onReconnect: () => taps++)));
    await tester.tap(find.text('Retry'));
    expect(taps, 1);
  });
}
