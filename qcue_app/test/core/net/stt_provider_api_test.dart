// QCue D4 — the multi-provider STT picker seam on QcueApiClient: the stub exposes the capability
// list and roundtrips an explicit selection (with "auto" ⇒ null / Auto).
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';

void main() {
  test('stub exposes capability list and roundtrips a selection', () async {
    final api = StubApiClient.seeded();

    final p = await api.sttProviders();
    expect(p.allCapable, contains('openai'));
    expect(p.allCapable, contains('zhipu'));
    expect(p.allCapable, contains('qwen'));
    expect(p.allCapable, isNot(contains('minimax')));
    expect(p.selected, isNull, reason: 'defaults to Auto');

    await api.setSttProvider('zhipu');
    expect((await api.sttProviders()).selected, 'zhipu');

    await api.setSttProvider('auto'); // Auto ⇒ null
    expect((await api.sttProviders()).selected, isNull);
  });
}
