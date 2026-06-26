// QCue S4-R48/R49: the settings repository wraps the model-list (fetch_models,
// TTL-cached, degrades to last-known on failure) and the privacy (D9 server-
// Dream) control.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/features/settings/settings_repository.dart';

class FakeSettingsApi implements SettingsApi {
  List<String>? modelsResult;
  bool failModels = false;
  bool? serverDream;
  final activePicks = <String, String>{};
  @override
  Future<List<String>> fetchModels(String provider) async {
    if (failModels) throw Exception('network');
    return modelsResult ?? ['m1', 'm2'];
  }

  @override
  Future<void> setActiveModel(String provider, String model) async =>
      activePicks[provider] = model;

  @override
  Future<void> setServerDream(bool on) async => serverDream = on;
}

void main() {
  test('S4-R48: model picker uses fetch_models, cached, graceful on failure',
      () async {
    final api = FakeSettingsApi()..modelsResult = ['gpt-x'];
    final repo = SettingsRepository(api);
    expect(await repo.models('openai'), ['gpt-x']);
    api.failModels = true; // refresh fails → fall back to last-known
    expect(await repo.models('openai'), ['gpt-x']);
  });

  test('S4-R48: a first-time failure degrades to an empty (not crashing) list',
      () async {
    final api = FakeSettingsApi()..failModels = true;
    final repo = SettingsRepository(api);
    expect(await repo.models('openai'), const <String>[]);
  });

  test('S4-R48: setActiveModel routes through the api', () async {
    final api = FakeSettingsApi();
    final repo = SettingsRepository(api);
    await repo.setActiveModel('openai', 'gpt-x');
    expect(api.activePicks['openai'], 'gpt-x');
  });

  test('S4-R49: privacy toggle controls server-side Dream', () async {
    final api = FakeSettingsApi();
    final repo = SettingsRepository(api);
    await repo.setServerDream(false);
    expect(api.serverDream, isFalse);
  });
}
