import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';

void main() {
  test('stub supports detail, edit, delete', () async {
    final api = StubApiClient.seeded();
    final created = await api.capture(body: 'hello', origin: 'capture');
    final detail = await api.captureDetail(created.id);
    expect(detail?.body, 'hello');
    await api.updateCapture(created.id, body: 'edited');
    expect((await api.captureDetail(created.id))?.body, 'edited');
    await api.deleteCapture(created.id);
    expect(await api.captureDetail(created.id), isNull);
    expect((await api.captures()).any((i) => i.id == created.id), isFalse);
  });
}
