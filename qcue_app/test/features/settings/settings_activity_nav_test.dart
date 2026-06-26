// v0.2.2: Activity moved from a bottom-nav tab into Settings. This asserts the
// Settings screen exposes a tappable "Activity" row that opens the full
// ActivityScreen at /settings/activity (within the Settings branch, so the
// bottom bar stays and Back returns to Settings).
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:qcue_app/core/router/qcue_router.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/activity/activity_screen.dart';

Widget _app(GoRouter router) => ProviderScope(
      child: MaterialApp.router(
        theme: QCueTheme.build(QThemeId.cleanLight),
        routerConfig: router,
      ),
    );

void main() {
  testWidgets('Settings has an Activity row that opens the Activity sub-page',
      (tester) async {
    final router = buildQcueRouter(initialLocation: '/settings');
    await tester.pumpWidget(_app(router));
    await tester.pumpAndSettle();

    final row = find.byKey(const ValueKey('settings-activity-row'));
    expect(row, findsOneWidget);

    await tester.tap(row);
    await tester.pumpAndSettle();

    // We navigated into the full Activity view, kept under Settings.
    expect(find.byType(ActivityScreen), findsOneWidget);
    final ctx = tester.element(find.byType(Navigator).last);
    expect(GoRouter.of(ctx).canPop(), isTrue); // Back → Settings, not app-exit
  });
}
