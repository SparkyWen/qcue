// QCue Sync Phase 1 (Task 12): the bootstrap host pulls the read-sync change
// feed on the read-sync triggers — first frame, connectivity→online, and
// app-resume (plus a periodic timer in production). This mirrors the existing
// flush-trigger wiring with a spy SyncClient, asserting pull() is invoked.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/offline/connectivity.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';
import 'package:qcue_app/core/sync/cache_revision.dart';
import 'package:qcue_app/core/sync/sync_client.dart';
import 'package:qcue_app/core/sync/sync_client_provider.dart';
import 'package:qcue_app/core/sync/sync_dtos.dart';
import 'package:qcue_app/main.dart';

/// A spy [SyncClient] that counts `pull()` invocations instead of touching a
/// network. register()/applyDelta are inherited but never reached here.
class _SpySyncClient extends SyncClient {
  _SpySyncClient()
      : super(
          registerDevice: (_) async =>
              const DeviceReg(deviceId: 'spy', siteId: 1),
          pullSync: ({required int since}) async => const SyncDelta(cursor: 0),
          cache: IdeaCache(InMemoryCacheStore(), feedCap: 10),
          platform: 'test',
        );

  int pulls = 0;

  /// What the next pull reports as "changed" (drives the cache-revision bump).
  bool nextChanged = false;
  @override
  Future<bool> pull() async {
    pulls++;
    return nextChanged;
  }
}

Widget _host(ProviderContainer container) => UncontrolledProviderScope(
      container: container,
      child: const ConnectivityHost(
        child: MaterialApp(home: SizedBox.shrink()),
      ),
    );

void main() {
  testWidgets('Task 12: pull() fires on first frame', (tester) async {
    final spy = _SpySyncClient();
    final container = ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(StubApiClient.seeded()),
      syncClientProvider.overrideWithValue(spy),
    ]);
    addTearDown(container.dispose);

    await tester.pumpWidget(_host(container));
    await tester.pump(); // let the post-frame callback run

    expect(spy.pulls, greaterThanOrEqualTo(1));
  });

  testWidgets('Task 12: pull() fires on connectivity → online', (tester) async {
    final spy = _SpySyncClient();
    final container = ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(StubApiClient.seeded()),
      syncClientProvider.overrideWithValue(spy),
      // Keep the readiness probe "unreachable" so the host's post-frame probe
      // does NOT flip us online — the manual setOnline() below is then the real
      // offline→online edge the host listens for.
      connectivitySourceProvider
          .overrideWithValue(FakeConnectivitySource(() async => false)),
    ]);
    addTearDown(container.dispose);

    // Start offline so the transition to online is a real edge the host listens
    // for; pump past the first-frame pull and record the baseline.
    container.read(connectivityProvider.notifier).setOffline();
    await tester.pumpWidget(_host(container));
    await tester.pump();
    final baseline = spy.pulls;

    container.read(connectivityProvider.notifier).setOnline();
    await tester.pump();

    expect(spy.pulls, greaterThan(baseline)); // online edge pulled
  });

  testWidgets('Task 12: pull() fires on app-resume', (tester) async {
    final spy = _SpySyncClient();
    final container = ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(StubApiClient.seeded()),
      syncClientProvider.overrideWithValue(spy),
    ]);
    addTearDown(container.dispose);

    await tester.pumpWidget(_host(container));
    await tester.pump();
    final baseline = spy.pulls;

    // Drive the lifecycle through the binding; the host is a registered observer.
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();

    expect(spy.pulls, greaterThan(baseline)); // resume pulled
  });

  testWidgets('a changed pull bumps the cache revision (refreshes the UI)',
      (tester) async {
    final spy = _SpySyncClient()..nextChanged = true;
    final container = ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(StubApiClient.seeded()),
      syncClientProvider.overrideWithValue(spy),
    ]);
    addTearDown(container.dispose);

    expect(container.read(cacheRevisionProvider), 0);
    await tester.pumpWidget(_host(container));
    await tester.pump(); // first-frame pull → changed → bump
    await tester.pump();

    expect(container.read(cacheRevisionProvider), greaterThan(0),
        reason: 'a delta-applying pull must bump so providers re-read');
  });

  testWidgets('a no-op pull does NOT bump the revision (no wasted re-fetch)',
      (tester) async {
    final spy = _SpySyncClient(); // nextChanged stays false
    final container = ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(StubApiClient.seeded()),
      syncClientProvider.overrideWithValue(spy),
    ]);
    addTearDown(container.dispose);

    await tester.pumpWidget(_host(container));
    await tester.pump();
    await tester.pump();

    expect(container.read(cacheRevisionProvider), 0,
        reason: 'an empty delta changes nothing, so the UI must not churn');
  });

  testWidgets('Task 12: inert when no SyncClient is wired (stub path)',
      (tester) async {
    // No syncClientProvider override → null → the host never pulls (no crash).
    final container = ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(StubApiClient.seeded()),
    ]);
    addTearDown(container.dispose);

    await tester.pumpWidget(_host(container));
    await tester.pump();
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    // Reaching here without throwing is the assertion (null SyncClient is inert).
    expect(container.read(syncClientProvider), isNull);
  });
}
