// QCue S4-R45: the 3-theme switcher. Selecting a theme swaps the resolved token
// map via the single themeProvider source of truth.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/core/theme/theme_provider.dart';
import 'package:qcue_app/features/settings/widgets/theme_switcher.dart';

void main() {
  testWidgets('S4-R45: selecting a theme swaps the active token map',
      (tester) async {
    final c = ProviderContainer();
    addTearDown(c.dispose);
    await tester.pumpWidget(UncontrolledProviderScope(
      container: c,
      child: Consumer(builder: (context, ref, _) {
        return MaterialApp(
          theme: QCueTheme.build(ref.watch(themeProvider)),
          home: const Scaffold(body: ThemeSwitcher()),
        );
      }),
    ));
    expect(c.read(themeProvider), QThemeId.cleanLight);
    await tester.tap(find.text('Night'));
    await tester.pumpAndSettle();
    expect(c.read(themeProvider), QThemeId.night);
    // selecting back works too
    await tester.tap(find.text('Anthropic Warm'));
    await tester.pumpAndSettle();
    expect(c.read(themeProvider), QThemeId.anthropicWarm);
  });

  testWidgets('S4-R45: the active theme is marked selected', (tester) async {
    final c = ProviderContainer();
    addTearDown(c.dispose);
    await tester.pumpWidget(UncontrolledProviderScope(
      container: c,
      child: Consumer(builder: (context, ref, _) {
        return MaterialApp(
          theme: QCueTheme.build(ref.watch(themeProvider)),
          home: const Scaffold(body: ThemeSwitcher()),
        );
      }),
    ));
    final selected = tester
        .widgetList<Semantics>(find.byType(Semantics))
        .where((s) => s.properties.selected == true);
    expect(selected, isNotEmpty);
  });
}
