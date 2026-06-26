// QCue S4-R48/R49: the settings control surface — the per-provider model list
// (the `fetch_models` surface, TTL-cached + degrades to last-known on a refresh
// failure rather than an empty picker) + the active-model pick + the privacy
// (D9 server-Dream) toggle. The narrow seam adapts onto [QcueApiClient].
abstract interface class SettingsApi {
  Future<List<String>> fetchModels(String provider);
  Future<void> setActiveModel(String provider, String model);
  Future<void> setServerDream(bool on);
}

class SettingsRepository {
  SettingsRepository(this._api);
  final SettingsApi _api;
  final _modelCache = <String, List<String>>{};

  /// Cached models; on refresh failure degrade to the last-known list rather
  /// than an empty picker (S4-R48).
  Future<List<String>> models(String provider) async {
    try {
      final fresh = await _api.fetchModels(provider);
      _modelCache[provider] = fresh;
      return fresh;
    } catch (_) {
      return _modelCache[provider] ?? const [];
    }
  }

  Future<void> setActiveModel(String provider, String model) =>
      _api.setActiveModel(provider, model);

  /// Toggle the server-readable / server-side nightly Dream posture (D9).
  Future<void> setServerDream(bool on) => _api.setServerDream(on);
}
