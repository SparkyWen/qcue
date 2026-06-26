// QCue cloud-sync fix (Task 4): a runtime server-URL override so the app can be
// pointed at a deployed server WITHOUT a rebuild. The build-time
// `--dart-define=QCUE_BASE_URL=...` still works as a default; this lets a user
// type a host in Settings and re-probe `/readyz` without re-flashing the binary.
//
// Persisted in [SharedPreferences] under `server_base_url`. The value is only
// honoured if it parses as an http(s) URL (see [QcueConfig.resolveBaseUrl]).
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// The runtime server-URL store. Overridden at bootstrap with a
/// SharedPreferences-backed instance; throws if read before that override (the
/// Settings screen only runs inside the booted app, where it IS overridden).
final serverUrlStoreProvider = Provider<ServerUrlStore>(
  (_) => throw UnimplementedError('serverUrlStoreProvider not overridden'),
);

/// Reads/writes the runtime server base-URL override in [SharedPreferences].
class ServerUrlStore {
  ServerUrlStore(this._prefs);

  /// The SharedPreferences key holding the runtime base-URL override.
  static const key = 'server_base_url';

  final SharedPreferences _prefs;

  /// The persisted override, or null if none has been set.
  String? read() {
    final v = _prefs.getString(key);
    if (v == null || v.trim().isEmpty) return null;
    return v.trim();
  }

  /// Persist a new override (trimmed). An empty string clears the override.
  Future<void> write(String url) async {
    final v = url.trim();
    if (v.isEmpty) {
      await _prefs.remove(key);
      return;
    }
    await _prefs.setString(key, v);
  }

  /// Clear the override (fall back to the build-time / default base URL).
  Future<void> clear() => _prefs.remove(key);
}
