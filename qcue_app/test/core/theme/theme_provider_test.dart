import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/core/theme/theme_provider.dart';

class FakeThemeStore implements ThemeStore {
  QThemeId? saved;
  @override
  QThemeId? load() => saved;
  @override
  void save(QThemeId id) => saved = id;
}

void main() {
  test('S4-R45: default is Clean Light; selection swaps + persists', () {
    final store = FakeThemeStore();
    final c = ProviderContainer(
      overrides: [themeStoreProvider.overrideWithValue(store)],
    );
    addTearDown(c.dispose);
    expect(c.read(themeProvider), QThemeId.cleanLight);
    c.read(themeProvider.notifier).select(QThemeId.night);
    expect(c.read(themeProvider), QThemeId.night);
    expect(store.saved, QThemeId.night); // persisted
  });

  test('S4-R45: a persisted choice restores on restart', () {
    final store = FakeThemeStore()..saved = QThemeId.anthropicWarm;
    final c = ProviderContainer(
      overrides: [themeStoreProvider.overrideWithValue(store)],
    );
    addTearDown(c.dispose);
    expect(c.read(themeProvider), QThemeId.anthropicWarm);
  });
}
