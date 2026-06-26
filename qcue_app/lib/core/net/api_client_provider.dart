// QCue S4: the single QcueApiClient seam as a Riverpod provider. The whole app
// reads its data through this one client. The foundation ships the seeded stub
// so the 3 content screens run against realistic content; the real WSS/SSE
// client overrides this provider at bootstrap in the next milestone.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../offline/connectivity.dart';
import 'qcue_api_client.dart';
import 'qcue_config.dart';

// Re-export the seam interface so features/widgets can name [QcueApiClient]
// (e.g. in a repository adapter) while importing ONLY this one bridge file —
// keeping the layering law (S4-R1: no feature reaches the raw transport).
export 'qcue_api_client.dart' show QcueApiClient, ApiConnectionState;
// The net/config types + the runtime server-URL store, re-exported through the
// one bridge so features (Settings, the auth repo) name them WITHOUT importing
// `core/net/*` directly — the layering law's sanctioned single seam (S4-R1).
export 'qcue_config.dart' show QcueConfig, TokenStore;
export 'server_url_store.dart' show ServerUrlStore, serverUrlStoreProvider;

/// The live network config (base URL + token store). Overridden at app bootstrap
/// with the resolved base URL; defaults to the cloud default so tests have a value.
/// Lives on the bridge because it is a net concern the whole app shares.
final qcueConfigProvider = Provider<QcueConfig>((_) => QcueConfig());

/// The durable token store. Overridden at bootstrap with the OS-secure
/// [SecureTokenStore]; defaults to an in-memory store (tests / pre-bootstrap).
final tokenStoreProvider = Provider<TokenStore>((_) => InMemoryTokenStore());

/// The active data client. Overridden at app bootstrap (real client) and in
/// tests (inert / seeded stub).
final apiClientProvider =
    Provider<QcueApiClient>((_) => StubApiClient.seeded());

/// Whether the app is currently offline (drives the offline banner + stale
/// markers). Now backed by the real [connectivityProvider] singleton so the
/// banner reflects actual connectivity. Stays a `Provider<bool>` so widget
/// tests can still override it directly with a literal.
final offlineProvider = Provider<bool>(
  (ref) => ref.watch(connectivityProvider) == Connectivity.offline,
);
