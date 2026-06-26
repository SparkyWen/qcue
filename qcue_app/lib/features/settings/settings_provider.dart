// QCue S4: the Settings screen state — loads the BYOK vault, the per-provider
// model lists + active picks, the cost ledger, and the D9 privacy posture
// through the single apiClientProvider seam. Mutations (putKey/deleteKey,
// setActiveModel, setServerDream) write back through the seam and refresh.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/location/capture_location_store.dart';
import '../../core/models/protocol_models.dart';
import '../../core/net/api_client_provider.dart';
import '../../core/session/auth_state.dart';
import '../../core/sync/cache_revision.dart';
import 'settings_repository.dart';

/// Adapts [QcueApiClient] onto the narrow [SettingsApi] (models + privacy).
class _ApiSettingsAdapter implements SettingsApi {
  _ApiSettingsAdapter(this._api);
  final QcueApiClient _api;
  @override
  Future<List<String>> fetchModels(String provider) =>
      _api.fetchModels(provider);
  @override
  Future<void> setActiveModel(String provider, String model) =>
      _api.setActiveModel(provider, model);
  @override
  Future<void> setServerDream(bool on) => _api.setServerDream(on);
}

final settingsRepositoryProvider = Provider<SettingsRepository>((ref) {
  ref.watch(cacheRevisionProvider); // ISO-R4: rebuild (fresh _modelCache) when the account changes
  return SettingsRepository(_ApiSettingsAdapter(ref.watch(apiClientProvider)));
});

/// The aggregate Settings snapshot for the screen.
class SettingsSnapshot {
  const SettingsSnapshot({
    required this.credentials,
    required this.models,
    required this.activeModels,
    required this.costLedger,
    required this.monthTotalMicros,
    required this.capMicros,
    required this.serverDream,
    required this.captureLocationEnabled,
  });
  final List<ProviderCredential> credentials;
  final Map<String, List<String>> models;
  final Map<String, String?> activeModels;
  final List<CostLedgerRow> costLedger;
  final int monthTotalMicros;
  final int capMicros;
  final bool serverDream;

  /// LOC-R2: whether new captures are tagged with the action-time GPS fix. A
  /// DEVICE-LOCAL toggle (off by default), read from [captureLocationStoreProvider].
  final bool captureLocationEnabled;
}

class SettingsNotifier extends AsyncNotifier<SettingsSnapshot> {
  @override
  Future<SettingsSnapshot> build() async {
    ref.watch(cacheRevisionProvider); // ISO-R4: re-fetch settings for the new account on a switch
    final api = ref.watch(apiClientProvider);
    final credentials = await api.credentials();
    final ledger = await api.costLedger();
    final serverDream = await api.serverDream();

    final models = <String, List<String>>{};
    final active = <String, String?>{};
    for (final c in credentials) {
      models[c.provider] = await api.fetchModels(c.provider);
      active[c.provider] = await api.activeModel(c.provider);
    }

    // Month total comes straight from the server-aggregated ledger (never summed
    // from messages.usage). The per-tenant cap is a fixed launch ceiling (D17).
    final monthTotal =
        ledger.fold<int>(0, (sum, r) => sum + r.costMicros);
    return SettingsSnapshot(
      credentials: credentials,
      models: models,
      activeModels: active,
      costLedger: ledger,
      monthTotalMicros: monthTotal,
      capMicros: 150000000, // $150.00 launch cap
      serverDream: serverDream,
      // Device-local toggle (off by default) — never round-trips to the server.
      captureLocationEnabled: ref.read(captureLocationStoreProvider).enabled,
    );
  }

  Future<void> putKey(String provider, String key) async {
    await ref.read(apiClientProvider).putKey(provider, key);
    state = await AsyncValue.guard(build);
  }

  Future<void> deleteKey(String provider) async {
    await ref.read(apiClientProvider).deleteKey(provider);
    state = await AsyncValue.guard(build);
  }

  Future<void> setActiveModel(String provider, String model) async {
    await ref.read(settingsRepositoryProvider).setActiveModel(provider, model);
    state = await AsyncValue.guard(build);
  }

  Future<void> setServerDream(bool on) async {
    await ref.read(settingsRepositoryProvider).setServerDream(on);
    state = await AsyncValue.guard(build);
  }

  /// LOC-R2: flip the device-local capture-location toggle (no server round-trip).
  Future<void> setCaptureLocation(bool on) async {
    await ref.read(captureLocationStoreProvider).setEnabled(on);
    state = await AsyncValue.guard(build);
  }

  /// Apple Guideline 5.1.1(v): permanently delete the account. The server purges
  /// the tenant + ALL synced data; then signOut() clears the durable tokens +
  /// local session (and, via the offline client, wipes the on-device cache), so
  /// the router redirect carries the user back to /login.
  Future<void> deleteAccount() async {
    await ref.read(apiClientProvider).deleteAccount();
    await ref.read(authStateProvider.notifier).signOut();
  }
}

final settingsProvider =
    AsyncNotifierProvider<SettingsNotifier, SettingsSnapshot>(
        SettingsNotifier.new);
