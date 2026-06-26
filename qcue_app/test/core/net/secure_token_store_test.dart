// QCue cloud-sync fix (Task 3): the durable token store. These tests pin the
// canonical guarantees:
//   - a written pair PERSISTS — a fresh store loaded over the same backing
//     rehydrates both tokens (the fix for "session lost on restart");
//   - accessSync reads the in-memory mirror synchronously (for the SSE `?token=`
//     callback that can't await);
//   - clear() wipes BOTH the mirror and the backing store.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/secure_token_store.dart';
import 'package:qcue_app/core/secure/secure_storage.dart';

/// An in-memory [SecureStorage] standing in for the OS Keychain/Keystore — the
/// "durable backing" a fresh store reloads from.
class _FakeSecure implements SecureStorage {
  final Map<String, String> _store = {};

  @override
  Future<void> write(String key, String value) async => _store[key] = value;

  @override
  Future<String?> read(String key, {String? reason}) async => _store[key];

  @override
  Future<void> delete(String key) async => _store.remove(key);
}

void main() {
  test('a fresh store loads the persisted pair (survives a "restart")',
      () async {
    final backing = _FakeSecure();
    final store = await SecureTokenStore.load(backing);
    expect(store.accessSync, isEmpty); // nothing yet

    await store.write(access: 'acc-1', refresh: 'ref-1');

    // A brand-new instance over the SAME backing rehydrates both tokens.
    final reloaded = await SecureTokenStore.load(backing);
    expect(reloaded.accessSync, 'acc-1');
    expect(await reloaded.readAccess(), 'acc-1');
    expect(await reloaded.readRefresh(), 'ref-1');
  });

  test('accessSync mirrors the latest write synchronously', () async {
    final store = await SecureTokenStore.load(_FakeSecure());
    expect(store.accessSync, ''); // empty before login
    await store.write(access: 'acc-2', refresh: 'ref-2');
    expect(store.accessSync, 'acc-2'); // no await needed
  });

  test('clear wipes the mirror AND the durable backing', () async {
    final backing = _FakeSecure();
    final store = await SecureTokenStore.load(backing);
    await store.write(access: 'acc-3', refresh: 'ref-3');

    await store.clear();
    expect(store.accessSync, isEmpty);
    expect(await store.readAccess(), isNull);
    expect(await store.readRefresh(), isNull);

    // A fresh load sees nothing — the backing was cleared, not just the mirror.
    final reloaded = await SecureTokenStore.load(backing);
    expect(reloaded.accessSync, isEmpty);
  });

  test('a backing-store read failure degrades to a signed-out store', () async {
    final store = await SecureTokenStore.load(_ThrowingSecure());
    expect(store.accessSync, isEmpty); // non-fatal: starts signed-out
  });

  test('SecureTokenStore persists and rehydrates the access expiry', () async {
    final backing = _FakeSecure();
    final store = await SecureTokenStore.load(backing);
    await store.write(access: 'a', refresh: 'r');
    final exp = DateTime.utc(2026, 6, 16, 10, 30);
    await store.writeExpiry(exp);

    final reloaded = await SecureTokenStore.load(backing);
    expect(reloaded.expiresAtSync, exp);
  });

  test('a transient read error is flagged (hadReadError) and is NOT an empty store',
      () async {
    final store = await SecureTokenStore.load(_ThrowingSecure());
    expect(store.accessSync, isEmpty); // can't read → no live token in mirror
    expect(store.hadReadError, isTrue,
        reason: 'a locked-Keychain read must be distinguishable');
  });

  test('a genuinely empty store is NOT flagged as a read error', () async {
    final store = await SecureTokenStore.load(_FakeSecure());
    expect(store.accessSync, isEmpty);
    expect(store.hadReadError, isFalse);
  });
}

/// A secure store whose reads throw (locked device / missing native module) —
/// the load must degrade to empty rather than crash the bootstrap.
class _ThrowingSecure implements SecureStorage {
  @override
  Future<void> write(String key, String value) async {}
  @override
  Future<String?> read(String key, {String? reason}) async =>
      throw Exception('locked');
  @override
  Future<void> delete(String key) async {}
}
