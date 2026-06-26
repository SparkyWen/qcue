// QCue S4-R36: the connections view shows the current page + its 1-hop
// neighbors (backlinks) as ≥44pt navigating nodes, plus a labeled-disabled
// "full graph — coming soon" entry.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/wiki/connections_view.dart';

Widget _host(Widget child) => ProviderScope(
      overrides: [apiClientProvider.overrideWithValue(StubApiClient.seeded())],
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: Scaffold(body: child),
      ),
    );

void main() {
  testWidgets('shows current page + a navigating backlink node + full-graph',
      (tester) async {
    String? navigated;
    await tester.pumpWidget(_host(
      ConnectionsView(slug: 'auto-dream', onOpenPage: (s) => navigated = s),
    ));
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('connections-view')), findsOneWidget);
    // the labeled-disabled "full graph — coming soon" entry (M5+)
    expect(
        find.byKey(const ValueKey('connections-full-graph')), findsOneWidget);
    expect(find.textContaining('coming soon'), findsOneWidget);

    // a 1-hop neighbor (backlink) node that navigates on tap (≥44pt).
    final neighbor =
        find.byKey(const ValueKey('connection-recall-architecture'));
    expect(neighbor, findsOneWidget);
    await tester.tap(neighbor);
    expect(navigated, 'recall-architecture');
  });

  testWidgets('a 44pt minimum tap target for each node', (tester) async {
    await tester.pumpWidget(_host(
      const ConnectionsView(slug: 'auto-dream'),
    ));
    await tester.pumpAndSettle();
    final box = tester.getSize(
      find.byKey(const ValueKey('connection-recall-architecture')),
    );
    expect(box.height, greaterThanOrEqualTo(44));
  });
}
