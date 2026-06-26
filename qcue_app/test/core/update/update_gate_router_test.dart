import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:qcue_app/core/router/qcue_router.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';

Widget _app(GoRouter r) => ProviderScope(
      child: MaterialApp.router(
        theme: QCueTheme.build(QThemeId.cleanLight),
        routerConfig: r,
      ),
    );

void main() {
  testWidgets('updateRequired blocks every route behind /update-required', (tester) async {
    final r = buildQcueRouter(isAuthed: () => true, isUpdateRequired: () => true);
    await tester.pumpWidget(_app(r));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('update-required')), findsOneWidget);
    expect(r.routerDelegate.currentConfiguration.uri.path, '/update-required');
  });

  testWidgets('no forced update ⇒ normal routing (no gate)', (tester) async {
    final r = buildQcueRouter(isAuthed: () => true, isUpdateRequired: () => false);
    await tester.pumpWidget(_app(r));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('update-required')), findsNothing);
    expect(r.routerDelegate.currentConfiguration.uri.path, '/capture');
  });
}
