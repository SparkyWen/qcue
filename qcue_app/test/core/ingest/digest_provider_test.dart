// QCue DIG-R6: the digest action is idle → running → done(count); errors surface as a failed state.
// (Relocated from features/wiki to core/ingest with the Digest button move to Settings.)
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/ingest/digest_provider.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';

void main() {
  test('runs and lands in DigestDone with the enqueued count', () async {
    final container = ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(StubApiClient.seeded()),
    ]);
    addTearDown(container.dispose);

    expect(container.read(digestProvider), isA<DigestIdle>());
    await container.read(digestProvider.notifier).run();
    final state = container.read(digestProvider);
    expect(state, isA<DigestDone>());
    // the seed has at least one pending idea → a non-negative count.
    expect((state as DigestDone).enqueued, greaterThanOrEqualTo(0));
  });
}
