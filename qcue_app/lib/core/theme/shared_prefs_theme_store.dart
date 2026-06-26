// QCue S4-R45: the production [ThemeStore] backed by SharedPreferences. Loaded
// synchronously at bootstrap (main.dart awaits the prefs handle), so the
// notifier's synchronous `load()` returns the persisted choice on first frame.
import 'package:shared_preferences/shared_preferences.dart';
import 'qcue_tokens.dart';
import 'theme_provider.dart';

class SharedPrefsThemeStore implements ThemeStore {
  SharedPrefsThemeStore(this._prefs);
  final SharedPreferences _prefs;

  static const _key = 'qcue.theme';

  @override
  QThemeId? load() {
    final name = _prefs.getString(_key);
    if (name == null) return null;
    for (final id in QThemeId.values) {
      if (id.name == name) return id;
    }
    return null;
  }

  @override
  void save(QThemeId id) => _prefs.setString(_key, id.name);
}
