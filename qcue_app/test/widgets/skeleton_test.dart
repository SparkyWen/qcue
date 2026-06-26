// QCue S4-R55: skeleton placeholders cross a 300ms gate (nothing before, the
// placeholder after) and collapse to a STATIC block under reduced motion — so a
// widget test never hangs on a perpetual animation. Tests run reduced-motion on
// purpose (the pulse path is intentionally not pumpAndSettle'd).
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/widgets/skeleton.dart';

Widget _host(Widget child, {bool reducedMotion = true}) => MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Builder(
        builder: (context) => MediaQuery(
          data:
              MediaQuery.of(context).copyWith(disableAnimations: reducedMotion),
          child: Scaffold(body: child),
        ),
      ),
    );

void main() {
  testWidgets('DelayedSkeleton: nothing before 300ms, child after (no flash)',
      (tester) async {
    await tester.pumpWidget(_host(const DelayedSkeleton(child: Text('SK'))));
    await tester.pump();
    expect(find.text('SK'), findsNothing);
    await tester.pump(const Duration(milliseconds: 350));
    expect(find.text('SK'), findsOneWidget);
  });

  testWidgets('Skeleton is a static block under reduced motion (settles)',
      (tester) async {
    await tester.pumpWidget(_host(const Skeleton(width: 100, height: 12)));
    await tester.pump();
    expect(
      find.descendant(
          of: find.byType(Skeleton), matching: find.byType(FadeTransition)),
      findsNothing,
    );
    // No perpetual animation → pumpAndSettle must return (not hang/OOM).
    await tester.pumpAndSettle();
    expect(find.byType(Skeleton), findsOneWidget);
  });

  testWidgets('SkeletonList renders placeholder rows', (tester) async {
    await tester.pumpWidget(_host(const SkeletonList(rows: 4)));
    await tester.pump();
    expect(find.byKey(const ValueKey('skeleton-list')), findsOneWidget);
    // 4 rows × 3 blocks each.
    expect(find.byType(Skeleton), findsNWidgets(12));
  });
}
