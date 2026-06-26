// QCue S4: the Settings screen body wires every section through the single
// apiClientProvider seam: theme switcher (kept), the BYOK vault (masked
// key_hint + health badge + add/delete), the model picker, the cost ledger
// (tabular), and the privacy (D9 server-Dream) toggle + sign-out. The vault NEVER
// shows a secret — only key_hint.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/settings/settings_screen.dart';
import 'package:qcue_app/features/settings/widgets/cost_ledger_table.dart';
import 'package:qcue_app/features/settings/widgets/health_badge.dart';
import 'package:qcue_app/features/settings/widgets/theme_switcher.dart';

Widget _app(StubApiClient api) => ProviderScope(
      overrides: [apiClientProvider.overrideWithValue(api)],
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const Scaffold(body: SettingsScreen()),
      ),
    );

void main() {
  // A tall viewport so the whole virtualized settings list renders for finds.
  setUp(() => TestWidgetsFlutterBinding.ensureInitialized());

  Future<void> pumpTall(WidgetTester tester, StubApiClient api) async {
    tester.view.physicalSize = const Size(1200, 4000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(_app(api));
    await tester.pumpAndSettle();
  }

  testWidgets('renders every section from the seeded seam', (tester) async {
    await pumpTall(tester, StubApiClient.seeded());
    expect(find.byType(ThemeSwitcher), findsOneWidget);
    expect(find.byType(HealthBadge), findsWidgets); // per-provider health
    expect(find.byType(CostLedgerTable), findsOneWidget);
    // section headers
    expect(find.text('Provider keys'), findsOneWidget);
    expect(find.text('Models'), findsOneWidget);
    expect(find.text('Usage & cost'), findsOneWidget);
    expect(find.text('Privacy'), findsOneWidget);
  });

  testWidgets('S4-R46: the vault lists masked key_hint, never a raw secret',
      (tester) async {
    await pumpTall(tester, StubApiClient.seeded());
    // the seeded openai hint is sk-…AB12 — surfaced; no full secret anywhere
    expect(find.textContaining('AB12'), findsOneWidget);
    expect(find.textContaining('…'), findsWidgets); // masked form
  });

  testWidgets('S4-R49: toggling the privacy switch flips server-Dream',
      (tester) async {
    final api = StubApiClient.seeded();
    expect(api.serverDreamEnabled, isTrue);
    await pumpTall(tester, api);
    await tester.tap(find.byKey(const ValueKey('server-dream-switch')));
    await tester.pumpAndSettle();
    expect(api.serverDreamEnabled, isFalse); // D9 posture changed
  });

  testWidgets('S4-R48: the model picker updates the active model', (tester) async {
    final api = StubApiClient.seeded();
    await pumpTall(tester, api);
    // open the openai model dropdown and pick the non-default (low-price) model
    await tester.tap(find.byKey(const ValueKey('model-picker-openai')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('gpt-5.4-mini').last);
    await tester.pumpAndSettle();
    expect(await api.activeModel('openai'), 'gpt-5.4-mini');
  });

  testWidgets('sign-out row is present', (tester) async {
    await pumpTall(tester, StubApiClient.seeded());
    expect(find.text('Sign out'), findsOneWidget);
  });
}
