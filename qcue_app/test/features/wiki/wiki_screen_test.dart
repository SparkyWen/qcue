// QCue S4-R34/R57: the Wiki browser. The index lists WikiPages grouped by
// wiki_page_type with title + one-line summary and a search filter; tapping a
// row navigates to that slug. The page view renders the markdown body with
// inline [[wikilinks]] (tappable, navigate to the slug), a quiet metadata line,
// and a Backlinks section. Unknown slug → page-not-found.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/wiki/wiki_page_screen.dart';
import 'package:qcue_app/features/wiki/wiki_screen.dart';

Widget _host(StubApiClient api, Widget child) => ProviderScope(
      overrides: [apiClientProvider.overrideWithValue(api)],
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: Scaffold(body: child),
      ),
    );

void main() {
  group('index', () {
    testWidgets('lists pages grouped by type with title + summary',
        (tester) async {
      String? navigated;
      await tester.pumpWidget(_host(
        StubApiClient.seeded(),
        WikiScreen(onOpenPage: (slug) => navigated = slug),
      ));
      await tester.pumpAndSettle();

      expect(find.text('Auto-Dream'), findsOneWidget);
      expect(find.text('Recall Architecture'), findsOneWidget);
      // a one-line summary is shown
      expect(
        find.textContaining('nightly consolidation pass'),
        findsOneWidget,
      );
      // group headers from wiki_page_type (rendered uppercased)
      expect(find.textContaining('CONCEPT'), findsWidgets);

      await tester.tap(find.text('Auto-Dream'));
      expect(navigated, 'auto-dream');
    });

    testWidgets('search filters the list', (tester) async {
      await tester.pumpWidget(_host(
        StubApiClient.seeded(),
        WikiScreen(onOpenPage: (_) {}),
      ));
      await tester.pumpAndSettle();

      await tester.enterText(
          find.byKey(const ValueKey('wiki-search')), 'recall');
      await tester.pumpAndSettle();
      expect(find.text('Recall Architecture'), findsOneWidget);
      expect(find.text('Approvals'), findsNothing);
    });

    testWidgets('empty index shows the empty state', (tester) async {
      await tester.pumpWidget(_host(
        StubApiClient(), // inert = no pages
        WikiScreen(onOpenPage: (_) {}),
      ));
      await tester.pumpAndSettle();
      expect(find.textContaining('No pages yet'), findsOneWidget);
    });
  });

  group('page view', () {
    testWidgets('renders the body + a tappable [[link]] that navigates',
        (tester) async {
      String? navigated;
      await tester.pumpWidget(_host(
        StubApiClient.seeded(),
        WikiPageScreen(slug: 'auto-dream', onOpenPage: (s) => navigated = s),
      ));
      await tester.pumpAndSettle();

      // body heading + prose rendered
      expect(find.text('Auto-Dream'), findsWidgets);
      // inline [[Recall Architecture]] link renders via QMarkdown + routes.
      final link = find.text('Recall Architecture', findRichText: true);
      expect(link, findsWidgets);
      await tester.ensureVisible(link.first);
      await tester.pumpAndSettle();
      await tester.tap(link.first);
      await tester.pumpAndSettle();
      expect(navigated, 'recall-architecture');
    });

    testWidgets('shows a quiet metadata line (type · updated · backlinks)',
        (tester) async {
      await tester.pumpWidget(_host(
        StubApiClient.seeded(),
        WikiPageScreen(slug: 'auto-dream', onOpenPage: (_) {}),
      ));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('wiki-meta')), findsOneWidget);
    });

    testWidgets('renders a Backlinks section listing linking pages',
        (tester) async {
      String? navigated;
      await tester.pumpWidget(_host(
        StubApiClient.seeded(),
        WikiPageScreen(slug: 'auto-dream', onOpenPage: (s) => navigated = s),
      ));
      await tester.pumpAndSettle();
      expect(find.text('Backlinks'), findsOneWidget);
      // a backlink to Recall Architecture is present and tappable
      expect(find.text('Recall Architecture'), findsWidgets);
      await tester.tap(find.byKey(const ValueKey('backlink-recall-architecture')));
      expect(navigated, 'recall-architecture');
    });

    testWidgets('unknown slug shows page-not-found', (tester) async {
      await tester.pumpWidget(_host(
        StubApiClient.seeded(),
        WikiPageScreen(slug: 'nope', onOpenPage: (_) {}),
      ));
      await tester.pumpAndSettle();
      expect(find.textContaining("hasn't been written"), findsOneWidget);
    });
  });
}
