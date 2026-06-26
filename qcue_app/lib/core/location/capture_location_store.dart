// Task 14c (LOC-R2): a DEVICE-LOCAL store for the "tag captures with location"
// toggle. Mirrors `core/net/server_url_store.dart` — a SharedPreferences-backed
// impl bound at bootstrap, plus an in-memory default so existing tests don't need
// the override. Off by default: a missing key reads `false`.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// The device-local capture-location toggle. NON-throwing in-memory default (so
/// existing Settings/provider tests keep working without an override); `main.dart`
/// overrides it with the SharedPreferences-backed instance.
final captureLocationStoreProvider =
    Provider<CaptureLocationStore>((ref) => InMemoryCaptureLocationStore());

/// Reads/writes whether new captures are tagged with the action-time GPS fix.
abstract interface class CaptureLocationStore {
  /// Whether location tagging is on. Default OFF.
  bool get enabled;

  /// Persist the toggle.
  Future<void> setEnabled(bool on);
}

/// The persisted toggle, backed by [SharedPreferences] (key
/// `capture_location_enabled`). A missing key reads `false` (off by default).
class SharedPrefsCaptureLocationStore implements CaptureLocationStore {
  SharedPrefsCaptureLocationStore(this._prefs);

  /// The SharedPreferences key holding the capture-location toggle.
  static const key = 'capture_location_enabled';

  final SharedPreferences _prefs;

  @override
  bool get enabled => _prefs.getBool(key) ?? false;

  @override
  Future<void> setEnabled(bool on) => _prefs.setBool(key, on);
}

/// An in-memory toggle (default OFF) for tests + the non-prod default.
class InMemoryCaptureLocationStore implements CaptureLocationStore {
  bool _enabled = false;

  @override
  bool get enabled => _enabled;

  @override
  Future<void> setEnabled(bool on) async => _enabled = on;
}
