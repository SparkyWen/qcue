// AU-R20 — device-local "automatic update check" toggle (default ON). Governs ONLY the manifest
// auto-check + nudge; never disables Shorebird OTA or the force gate. Mirrors the tokenStoreProvider
// pattern: a benign in-memory default (so tests + the keyless build just work), overridden at bootstrap
// with the durable SharedPreferences-backed store.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

abstract interface class UpdatePrefsStore {
  bool get autoCheckEnabled;
  Future<void> setAutoCheck(bool on);
}

/// Durable store backed by SharedPreferences (the real-device + demo path).
class SharedPrefsUpdatePrefsStore implements UpdatePrefsStore {
  SharedPrefsUpdatePrefsStore(this._prefs);
  final SharedPreferences _prefs;
  static const _key = 'qcue.update.autoCheck';
  @override
  bool get autoCheckEnabled => _prefs.getBool(_key) ?? true;
  @override
  Future<void> setAutoCheck(bool on) => _prefs.setBool(_key, on);
}

/// In-memory default (tests / pre-bootstrap). Auto-check on by default.
class InMemoryUpdatePrefsStore implements UpdatePrefsStore {
  InMemoryUpdatePrefsStore({bool autoCheck = true}) : _on = autoCheck;
  bool _on;
  @override
  bool get autoCheckEnabled => _on;
  @override
  Future<void> setAutoCheck(bool on) async => _on = on;
}

/// In-memory by default; overridden at bootstrap with [SharedPrefsUpdatePrefsStore].
final updatePrefsStoreProvider =
    Provider<UpdatePrefsStore>((_) => InMemoryUpdatePrefsStore());
