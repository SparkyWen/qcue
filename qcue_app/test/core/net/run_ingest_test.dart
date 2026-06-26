// QCue DIG-R6: the runIngest() seam returns the enqueued count; the stub answers keyless.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';

void main() {
  test('StubApiClient.runIngest returns a non-negative enqueued count', () async {
    final api = StubApiClient.seeded();
    final n = await api.runIngest();
    expect(n, greaterThanOrEqualTo(0));
  });
}
