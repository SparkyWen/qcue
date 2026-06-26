// QCue S4-R64: connectivity is a single shared source of truth that the offline
// banner + the OfflineAwareApiClient.flushOutbox both read. It can be driven by
// an injected [ConnectivitySource] (a `/readyz` reachability probe in production,
// a fake in tests) and exposes manual online/offline transitions for the banner.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/offline/connectivity.dart';

void main() {
  test('S4-R64: connectivity is a single shared instance', () {
    final c = ProviderContainer();
    addTearDown(c.dispose);
    final a = c.read(connectivityProvider.notifier);
    final b = c.read(connectivityProvider.notifier);
    expect(identical(a, b), isTrue);
  });

  test('connectivity transitions online<->offline', () {
    final c = ProviderContainer();
    addTearDown(c.dispose);
    expect(c.read(connectivityProvider), Connectivity.online);
    c.read(connectivityProvider.notifier).setOffline();
    expect(c.read(connectivityProvider), Connectivity.offline);
    c.read(connectivityProvider.notifier).setOnline();
    expect(c.read(connectivityProvider), Connectivity.online);
  });

  test('reachable() flips the state from a ConnectivitySource probe', () async {
    var reachable = false;
    final source = FakeConnectivitySource(() async => reachable);
    final c = ProviderContainer(
      overrides: [connectivitySourceProvider.overrideWithValue(source)],
    );
    addTearDown(c.dispose);
    final notifier = c.read(connectivityProvider.notifier);

    await notifier.probe();
    expect(c.read(connectivityProvider), Connectivity.offline);

    reachable = true;
    await notifier.probe();
    expect(c.read(connectivityProvider), Connectivity.online);
  });

  test('PingConnectivitySource maps an exception to unreachable', () async {
    final source = PingConnectivitySource(
      ping: () async => throw Exception('no route'),
    );
    expect(await source.isReachable(), isFalse);
  });

  test('PingConnectivitySource maps a 2xx readyz to reachable', () async {
    final source = PingConnectivitySource(ping: () async => 200);
    expect(await source.isReachable(), isTrue);
    final down = PingConnectivitySource(ping: () async => 503);
    expect(await down.isReachable(), isFalse);
  });
}
