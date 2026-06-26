import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/recall/recall_screen.dart';

Widget _app() => ProviderScope(
      overrides: [apiClientProvider.overrideWithValue(StubApiClient.seeded())],
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const Scaffold(body: RecallScreen()),
      ),
    );

void main() {
  testWidgets('history drawer lists conversations and reopening loads prior turns', (tester) async {
    await tester.pumpWidget(_app());
    await tester.pumpAndSettle();

    // open the drawer via the recall history button.
    await tester.tap(find.byKey(const ValueKey('recall-history-button')));
    await tester.pumpAndSettle();

    // the seeded conversation row renders (Data state).
    expect(find.text('What did I decide about embeddings?'), findsWidgets);

    // tapping the row reopens it → prior turns render in the conversation.
    await tester.tap(find.byKey(const ValueKey('convo-row-th-seed-1')));
    await tester.pumpAndSettle();
    expect(find.textContaining('You chose grep recall over vectors'), findsOneWidget);
  });

  testWidgets('the ＋ new action resets to the empty state', (tester) async {
    await tester.pumpWidget(_app());
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('recall-history-button')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('recall-new-button')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('recall-empty')), findsOneWidget);
  });
}
