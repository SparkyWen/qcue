import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';
import 'package:qcue_app/core/offline/offline_api_client.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/session/auth_state.dart';

void main() {
  test('signOut wipes the on-device cache', () async {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 200);
    cache.enqueueCapture(body: 'account-A-secret', origin: 'test');
    final api = OfflineAwareApiClient(StubApiClient.seeded(), cache);
    final c = ProviderContainer(
      overrides: [apiClientProvider.overrideWithValue(api)],
    );
    addTearDown(c.dispose);

    expect(cache.feed(), isNotEmpty);
    await c.read(authStateProvider.notifier).signOut();
    expect(cache.feed(), isEmpty, reason: 'logout must clear the cache');
  });
}
