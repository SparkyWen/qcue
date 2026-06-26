// QCue S4-R64: the single source of truth for connectivity. The OfflineBanner
// and the OfflineAwareApiClient.flushOutbox both read this one provider so the
// banner and the outbound flush stay in lockstep.
//
// Connectivity can be driven three ways:
//   - explicit transitions (`setOnline`/`setOffline`) — e.g. a hard network
//     failure inside the api client flips us offline immediately;
//   - a `/readyz` reachability probe via an injected [ConnectivitySource]
//     (the production impl is [PingConnectivitySource]); a fake is injected in
//     tests so no real socket is opened headlessly.
//
// We deliberately use a lightweight reachability probe rather than
// `connectivity_plus`: the platform-channel plugin does not run under headless
// `flutter test` on this host, and "the radio is up" is not the same as "the
// app-server is reachable". The probe answers the question the UI actually asks.
import 'package:flutter_riverpod/flutter_riverpod.dart';

enum Connectivity { online, offline }

/// The reachability seam. Production: a `/readyz` HTTP ping. Tests: a fake.
abstract interface class ConnectivitySource {
  /// True when the app-server is reachable (a 2xx from `/readyz`).
  Future<bool> isReachable();
}

/// A reachability probe over an injected `ping` (the actual HTTP GET to
/// `/readyz`, supplied by the bootstrap so this layer holds no `http` import).
/// Any thrown error (DNS, connection refused, timeout) means unreachable.
class PingConnectivitySource implements ConnectivitySource {
  PingConnectivitySource({required this.ping});

  /// The actual `/readyz` HTTP GET, returning the response status code. Supplied
  /// by the bootstrap so this layer holds no `http` import.
  final Future<int> Function() ping;

  @override
  Future<bool> isReachable() async {
    try {
      final status = await ping();
      return status >= 200 && status < 300;
    } catch (_) {
      return false;
    }
  }
}

/// A deterministic fake for tests.
class FakeConnectivitySource implements ConnectivitySource {
  FakeConnectivitySource(this._reachable);
  final Future<bool> Function() _reachable;
  @override
  Future<bool> isReachable() => _reachable();
}

/// The injectable reachability source. Defaults to "always reachable" so unit
/// tests that don't care about probing stay online; the bootstrap overrides it
/// with a real [PingConnectivitySource], tests with a [FakeConnectivitySource].
final connectivitySourceProvider = Provider<ConnectivitySource>(
  (_) => FakeConnectivitySource(() async => true),
);

class ConnectivityNotifier extends Notifier<Connectivity> {
  @override
  Connectivity build() => Connectivity.online;

  void setOnline() => state = Connectivity.online;
  void setOffline() => state = Connectivity.offline;

  /// Run the reachability probe and update the state accordingly.
  Future<void> probe() async {
    final ok = await ref.read(connectivitySourceProvider).isReachable();
    state = ok ? Connectivity.online : Connectivity.offline;
  }
}

/// Single source of truth for connectivity (S4-R64).
final connectivityProvider =
    NotifierProvider<ConnectivityNotifier, Connectivity>(
        ConnectivityNotifier.new);
