import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/sync/cache_revision.dart';
import 'package:qcue_app/features/settings/settings_provider.dart';

void main() {
  test('bumping the cache revision rebuilds the settings repository (clears its model cache)', () {
    final c = ProviderContainer();
    addTearDown(c.dispose);
    final r1 = c.read(settingsRepositoryProvider);
    c.read(cacheRevisionProvider.notifier).bump();
    final r2 = c.read(settingsRepositoryProvider);
    expect(identical(r1, r2), isFalse,
        reason: 'a different account must not reuse the prior SettingsRepository (modelCache leak)');
  });
}
