// QCue CAP-R1: captureDetailProvider resolves to Data(idea) for a known id,
// and Empty() when the id is absent.
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:qcue_app/core/models/screen_state.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/features/capture/capture_provider.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';

void main() {
  test('captureDetailProvider returns Data for an existing capture', () async {
    final api = StubApiClient.seeded();
    final created = await api.capture(body: 'x', origin: 'capture');
    final c = ProviderContainer(overrides: [apiClientProvider.overrideWithValue(api)]);
    addTearDown(c.dispose);
    final state = await c.read(captureDetailProvider(created.id).future);
    expect(state, isA<Data<Idea>>());
    expect((state as Data<Idea>).value.body, 'x');
  });
}
