// QCue — StubApiClient.deleteAccount wipes the in-memory account data, so the
// keyless/demo stack and widget tests can assert the "account gone" outcome.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';

void main() {
  test('StubApiClient.deleteAccount wipes credentials + feed', () async {
    final api = StubApiClient.seeded();
    await api.putKey('openai', 'sk-AAAA');
    expect(await api.credentials(), isNotEmpty);
    expect(await api.captures(), isNotEmpty);

    await api.deleteAccount();

    expect(await api.credentials(), isEmpty, reason: 'credentials wiped');
    expect(await api.captures(), isEmpty, reason: 'feed wiped');
  });

  test('StubApiClient.deleteAccount never throws on the inert stub', () async {
    // The inert stub backs several collections with const literals; deleteAccount
    // must not throw when clearing them.
    await StubApiClient().deleteAccount();
  });
}
