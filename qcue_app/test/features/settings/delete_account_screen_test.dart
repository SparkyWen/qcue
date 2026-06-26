// QCue — Apple Guideline 5.1.1(v): Settings → "Delete account" → destructive
// confirm → server-side purge (api.deleteAccount) → full auth/session teardown
// (signOut → unauthed → router redirects to /login). The local cache wipe is
// covered by offline_api_client_test; here we pin the screen + notifier wiring,
// the failure UX, and sign-out resilience.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/app_release_manifest.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/net/qcue_config.dart';
import 'package:qcue_app/core/session/auth_state.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/settings/settings_provider.dart';
import 'package:qcue_app/features/settings/settings_screen.dart';

Widget _app(QcueApiClient api) => ProviderScope(
      overrides: [
        apiClientProvider.overrideWithValue(api),
        // A held access token → AuthStateNotifier starts `authed`, so a successful
        // delete is observable as a flip to `unauthed`.
        tokenStoreProvider.overrideWithValue(InMemoryTokenStore(access: 'tok')),
      ],
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const Scaffold(body: SettingsScreen()),
      ),
    );

/// Scroll the (lazy) settings list until the destructive row is built + visible,
/// instead of relying on a hand-tuned tall viewport (robust to fixture growth).
Future<void> _revealDeleteRow(WidgetTester tester) async {
  await tester.scrollUntilVisible(
    find.text('Delete account'),
    300,
    scrollable: find.byType(Scrollable).first,
  );
}

void main() {
  test('SettingsNotifier.deleteAccount purges server-side then signs out',
      () async {
    final api = StubApiClient.seeded();
    await api.putKey('openai', 'sk-AAAA');
    final container = ProviderContainer(overrides: [
      apiClientProvider.overrideWithValue(api),
      tokenStoreProvider.overrideWithValue(InMemoryTokenStore(access: 'tok')),
    ]);
    addTearDown(container.dispose);
    expect(container.read(authStateProvider), AuthStatus.authed);
    await container.read(settingsProvider.future);

    await container.read(settingsProvider.notifier).deleteAccount();

    expect(await api.credentials(), isEmpty,
        reason: 'server-side account delete was called');
    expect(container.read(authStateProvider), AuthStatus.unauthed,
        reason: 'tokens/session cleared → router will bounce to /login');
  });

  testWidgets('Delete account row → confirm dialog → deletes the account',
      (tester) async {
    final api = StubApiClient.seeded();
    await api.putKey('openai', 'sk-AAAA');
    await tester.pumpWidget(_app(api));
    await tester.pumpAndSettle();

    await _revealDeleteRow(tester);
    await tester.tap(find.text('Delete account'));
    await tester.pumpAndSettle();

    // Confirm dialog spells out the irreversible consequence.
    expect(find.textContaining('permanently'), findsOneWidget);
    expect(find.text('Cancel'), findsOneWidget);
    // The confirm button is "Delete" (distinct from the row's "Delete account");
    // find.text is exact-match so this targets only the button.
    await tester.tap(find.text('Delete'));
    await tester.pumpAndSettle();

    expect(await api.credentials(), isEmpty,
        reason: 'deleteAccount reached the seam → stub account data wiped');
  });

  testWidgets('a failed server delete surfaces an error and does NOT sign out',
      (tester) async {
    final api = _ThrowingDeleteApi();
    await tester.pumpWidget(_app(api));
    await tester.pumpAndSettle();

    await _revealDeleteRow(tester);
    await tester.tap(find.text('Delete account'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Delete'));
    await tester.pumpAndSettle();

    // The failure is surfaced (not a silent no-op the user mistakes for success).
    expect(find.textContaining("Couldn't delete"), findsOneWidget);
  });

  test('signOut still lands unauthed even if the token store throws on clear',
      () async {
    // After a SUCCESSFUL server delete, signOut() must not leave the user wedged
    // in an authed shell if the local Keychain/Keystore wipe hiccups.
    final container = ProviderContainer(overrides: [
      tokenStoreProvider.overrideWithValue(_ThrowingClearTokenStore()),
    ]);
    addTearDown(container.dispose);
    expect(container.read(authStateProvider), AuthStatus.authed);

    await container.read(authStateProvider.notifier).signOut();

    expect(container.read(authStateProvider), AuthStatus.unauthed,
        reason: 'router must still bounce to /login after the server delete');
  });
}

/// A client whose account delete always fails (network down / 500), delegating
/// the settings reads to a seeded stub so the screen still builds.
class _ThrowingDeleteApi implements QcueApiClient {
  final StubApiClient _stub = StubApiClient.seeded();
  @override
  Future<void> deleteAccount() async => throw Exception('network down');
  @override
  Future<List<ProviderCredential>> credentials() => _stub.credentials();
  @override
  Future<List<CostLedgerRow>> costLedger() => _stub.costLedger();
  @override
  Future<bool> serverDream() => _stub.serverDream();
  @override
  Future<List<String>> fetchModels(String provider) =>
      _stub.fetchModels(provider);
  @override
  Future<String?> activeModel(String provider) => _stub.activeModel(provider);
  @override
  Future<AppReleaseManifest> fetchReleaseManifest(String platform) =>
      _stub.fetchReleaseManifest(platform); // the Settings 'Software update' section reads this
  @override
  dynamic noSuchMethod(Invocation invocation) =>
      super.noSuchMethod(invocation);
}

/// A token store that throws when asked to clear() — simulates a Keychain /
/// Keystore failure during sign-out.
class _ThrowingClearTokenStore implements TokenStore {
  final InMemoryTokenStore _inner = InMemoryTokenStore(access: 'tok');
  @override
  String get accessSync => _inner.accessSync;
  @override
  DateTime? get expiresAtSync => _inner.expiresAtSync;
  @override
  Future<String?> readAccess() => _inner.readAccess();
  @override
  Future<String?> readRefresh() => _inner.readRefresh();
  @override
  Future<void> write({required String access, required String refresh}) =>
      _inner.write(access: access, refresh: refresh);
  @override
  Future<void> writeExpiry(DateTime expiresAt) => _inner.writeExpiry(expiresAt);
  @override
  Future<void> clear() async => throw Exception('keystore delete failed');
}
