// QCue: the Capture screen gains a calendar "browse by day" button. Picking a date shows ALL of that
// day's captures (a date chip + day feed); clearing returns to the live newest-first feed.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/capture/capture_date_provider.dart';
import 'package:qcue_app/features/capture/capture_screen.dart';

ProviderContainer _container(QcueApiClient api, {bool offline = false}) =>
    ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(api),
      offlineProvider.overrideWith((ref) => offline),
    ]);

Widget _host(ProviderContainer c) => UncontrolledProviderScope(
      container: c,
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const Scaffold(body: CaptureScreen()),
      ),
    );

void main() {
  testWidgets('the Capture screen shows a calendar (browse-by-day) button',
      (tester) async {
    final c = _container(StubApiClient.seeded());
    addTearDown(c.dispose);
    await tester.pumpWidget(_host(c));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('capture-calendar')), findsOneWidget);
  });

  testWidgets('tapping the calendar opens a date picker', (tester) async {
    final c = _container(StubApiClient.seeded());
    addTearDown(c.dispose);
    await tester.pumpWidget(_host(c));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const ValueKey('capture-calendar')));
    await tester.pumpAndSettle();
    expect(find.byType(DatePickerDialog), findsOneWidget);
  });

  testWidgets("selecting a day shows the date chip + that day's feed; clearing returns to live",
      (tester) async {
    final api = StubApiClient.seeded();
    await api.capture(body: 'today only', origin: 'capture'); // captured today
    final c = _container(api);
    addTearDown(c.dispose);
    await tester.pumpWidget(_host(c));
    await tester.pumpAndSettle();

    // live feed shows the just-made capture.
    expect(find.text('today only'), findsOneWidget);

    // pick a far-past day with no captures.
    c.read(selectedCaptureDateProvider.notifier).select(DateTime(2001, 1, 1));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('capture-date-chip')), findsOneWidget);
    expect(find.text('today only'), findsNothing,
        reason: 'the day view shows only the selected day (empty here)');

    // clear → back to the live feed.
    c.read(selectedCaptureDateProvider.notifier).clear();
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('capture-date-chip')), findsNothing);
    expect(find.text('today only'), findsOneWidget);
  });

  testWidgets('committing while viewing today surfaces the new capture in the day view',
      (tester) async {
    final c = _container(StubApiClient.seeded());
    addTearDown(c.dispose);
    await tester.pumpWidget(_host(c));
    await tester.pumpAndSettle();

    // browse today, then capture a new thought from the always-visible field.
    c.read(selectedCaptureDateProvider.notifier).select(DateTime.now());
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('capture-date-chip')), findsOneWidget);

    await tester.enterText(
        find.byKey(const ValueKey('capture-field')), 'note while browsing today');
    await tester.testTextInput.receiveAction(TextInputAction.send);
    await tester.pumpAndSettle();

    expect(find.text('note while browsing today'), findsOneWidget,
        reason: "a capture made while viewing today must appear in today's day view");
  });
}
