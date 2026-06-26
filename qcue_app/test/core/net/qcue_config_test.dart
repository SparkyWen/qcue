import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_config.dart';

void main() {
  test('InMemoryTokenStore round-trips the access expiry', () async {
    final store = InMemoryTokenStore();
    expect(store.expiresAtSync, isNull); // none before write
    await store.write(access: 'a', refresh: 'r');
    final exp = DateTime.utc(2026, 6, 16, 10, 30);
    await store.writeExpiry(exp);
    expect(store.expiresAtSync, exp);
  });
}
