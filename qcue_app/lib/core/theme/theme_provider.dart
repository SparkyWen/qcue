// QCue S4-R45/R64: the active theme is a single Riverpod source of truth.
// Selection persists through an injectable [ThemeStore] (SharedPreferences in
// prod). Default is Clean Light.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'qcue_haptics.dart';
import 'qcue_tokens.dart';

/// Persistence seam (SharedPreferences in prod; injectable for tests).
abstract interface class ThemeStore {
  QThemeId? load();
  void save(QThemeId id);
}

class _NullThemeStore implements ThemeStore {
  const _NullThemeStore();
  @override
  QThemeId? load() => null;
  @override
  void save(QThemeId id) {}
}

final themeStoreProvider = Provider<ThemeStore>((_) => const _NullThemeStore());

class ThemeNotifier extends Notifier<QThemeId> {
  @override
  QThemeId build() => ref.read(themeStoreProvider).load() ?? QThemeId.cleanLight;

  void select(QThemeId id) {
    ref.read(themeStoreProvider).save(id);
    state = id;
  }
}

/// Single source of truth for the active theme (S4-R45/R64).
final themeProvider =
    NotifierProvider<ThemeNotifier, QThemeId>(ThemeNotifier.new);

/// The haptics sink (PlatformHaptics in prod; spy/no-op in tests) and the
/// Haptics helper that fires on the three key moments (S4-R54).
final hapticsSinkProvider = Provider<HapticsSink>((_) => const PlatformHaptics());
final hapticsProvider = Provider<Haptics>((ref) => Haptics(ref.watch(hapticsSinkProvider)));
