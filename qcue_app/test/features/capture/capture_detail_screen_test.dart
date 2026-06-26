// QCue CAP-R1/R2/R3: the capture detail screen — tap a feed row to inspect its
// body, captured-at time, location, and ingest state, then Edit or Delete.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/router/qcue_router.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/capture/capture_detail_screen.dart';

// Minimal harness (no router) for tests that don't navigate.
Widget _app(StubApiClient api, {required Widget child}) {
  return ProviderScope(
    overrides: [apiClientProvider.overrideWithValue(api)],
    child: MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(body: child),
    ),
  );
}

// Real-router harness: starts at /capture and lets the screen `context.pop()`
// back, exactly as in production (delete navigates away).
Widget _routedApp(StubApiClient api, String id) {
  return ProviderScope(
    overrides: [apiClientProvider.overrideWithValue(api)],
    child: MaterialApp.router(
      theme: QCueTheme.build(QThemeId.cleanLight),
      routerConfig: buildQcueRouter(initialLocation: '/capture/$id'),
    ),
  );
}

void main() {
  testWidgets('CAP-R1: detail screen shows body + a delete action', (
    tester,
  ) async {
    final api = StubApiClient.seeded();
    final created = await api.capture(body: 'detail body', origin: 'capture');
    await tester.pumpWidget(
      _app(api, child: CaptureDetailScreen(id: created.id)),
    );
    await tester.pumpAndSettle();

    expect(find.text('detail body'), findsOneWidget);
    expect(find.byKey(const ValueKey('capture-edit')), findsOneWidget);
    expect(find.byKey(const ValueKey('capture-delete')), findsOneWidget);
    // Time + (no) location meta lines render.
    expect(find.textContaining('No location'), findsOneWidget);
  });

  testWidgets('CAP-R1: a missing capture renders the gone state', (
    tester,
  ) async {
    final api = StubApiClient.seeded();
    await tester.pumpWidget(
      _app(api, child: const CaptureDetailScreen(id: 'nope')),
    );
    await tester.pumpAndSettle();
    expect(find.text('detail body'), findsNothing);
    expect(find.byKey(const ValueKey('capture-delete')), findsNothing);
  });

  testWidgets('CAP-R2: Edit dialog saves a changed body via the notifier', (
    tester,
  ) async {
    final api = StubApiClient.seeded();
    final created = await api.capture(body: 'before', origin: 'capture');
    await tester.pumpWidget(
      _app(api, child: CaptureDetailScreen(id: created.id)),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const ValueKey('capture-edit')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey('capture-edit-field')),
      'after',
    );
    await tester.tap(find.byKey(const ValueKey('capture-edit-save')));
    await tester.pumpAndSettle();

    expect((await api.captureDetail(created.id))!.body, 'after');
  });

  testWidgets(
    'CAP-R3: Delete confirm removes the capture + pops back to the feed',
    (tester) async {
      final api = StubApiClient.seeded();
      final created = await api.capture(body: 'doomed', origin: 'capture');
      await tester.pumpWidget(_routedApp(api, created.id));
      await tester.pumpAndSettle();
      expect(find.text('doomed'), findsOneWidget);

      await tester.tap(find.byKey(const ValueKey('capture-delete')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('capture-delete-confirm')));
      await tester.pumpAndSettle();

      // The capture is gone and we've popped off the detail route.
      expect(await api.captureDetail(created.id), isNull);
      expect(find.byKey(const ValueKey('capture-delete')), findsNothing);
    },
  );

  testWidgets('CAP-R1: tapping a feed row navigates to its detail', (
    tester,
  ) async {
    final api = StubApiClient.seeded();
    final created = await api.capture(body: 'tap me open', origin: 'capture');
    await tester.pumpWidget(
      ProviderScope(
        overrides: [apiClientProvider.overrideWithValue(api)],
        child: MaterialApp.router(
          theme: QCueTheme.build(QThemeId.cleanLight),
          routerConfig: buildQcueRouter(initialLocation: '/capture'),
        ),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(ValueKey('feed-row-${created.id}')));
    await tester.pumpAndSettle();

    // On the detail screen now: edit/delete actions are present.
    expect(find.byKey(const ValueKey('capture-edit')), findsOneWidget);
    expect(find.byKey(const ValueKey('capture-delete')), findsOneWidget);
  });
}
