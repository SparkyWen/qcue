// QCue S4 bootstrap. Loads the persisted theme (SharedPreferences) and the font
// loaders, then runs the app inside a ProviderScope. The single data seam
// ([apiClientProvider]) is bound here to the REAL network client wrapped by the
// offline decorator: [OfflineAwareApiClient] over ([HttpApiClient] + an
// [IdeaCache] backed by sqlite3) — so capture works offline and reads degrade to
// the cache (master §10; D5/D6 local-first). `--dart-define=QCUE_STUB=true`
// selects the seeded [StubApiClient] instead. Connectivity is a `/readyz`
// reachability probe ([connectivitySourceProvider]); on reconnect + app resume
// the outbound capture queue is flushed.
import 'dart:async';
import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;
import 'package:package_info_plus/package_info_plus.dart';
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'app.dart';
import 'background/flush_entrypoint.dart' as bg_flush;
import 'core/native/background/background_flush.dart';
import 'core/native/notif/notif_channel.dart';
import 'core/native/protocol/native_dtos.dart';
import 'core/native/secure/native_secure_storage.dart';
import 'core/native/audio/record_package_recorder.dart';
import 'core/native/codepush/code_push_facade.dart';
import 'core/native/installer/installer_channel.dart';
import 'core/native/share/share_channel.dart';
import 'core/native/widget/widget_channel.dart';
import 'core/location/capture_location_store.dart';
import 'core/location/location_service.dart';
import 'core/router/qcue_router.dart';
import 'core/net/api_client_provider.dart';
import 'core/net/http_api_client.dart';
import 'core/net/jwt_claims.dart';
import 'core/net/qcue_api_client.dart';
// QcueConfig/TokenStore + ServerUrlStore/serverUrlStoreProvider come via the bridge (api_client_provider).
import 'core/net/secure_token_store.dart';
import 'core/offline/connectivity.dart';
import 'core/offline/idea_cache.dart';
import 'core/offline/offline_api_client.dart';
import 'core/offline/sqlite_cache_store.dart';
import 'core/offline/sync_status.dart';
import 'core/offline/today_count.dart';
import 'core/secure/secure_storage_provider.dart';
import 'core/sync/cache_revision.dart';
import 'core/sync/sync_client.dart';
import 'core/sync/sync_client_provider.dart';
import 'core/theme/qcue_text.dart';
import 'core/theme/shared_prefs_theme_store.dart';
import 'core/theme/theme_provider.dart';
import 'features/auth/auth_provider.dart';
import 'features/auth/proactive_refresh.dart';
import 'features/capture/widgets/native_voice_capture_controller.dart';
import 'features/capture/widgets/voice_capture_controller.dart';
import 'features/onboarding/onboarding_store.dart';
import 'core/update/update_prefs_store.dart';

/// Demo/offline switch: `flutter run --dart-define=QCUE_STUB=true` keeps the app
/// on the seeded in-memory stub (no backend needed).
const _useStub = bool.fromEnvironment('QCUE_STUB');

/// The app-server base URL (override at build time, e.g. for a staging host):
/// `--dart-define=QCUE_BASE_URL=https://api.qcue.app`.
const _baseUrl = String.fromEnvironment('QCUE_BASE_URL',
    defaultValue: QcueConfig.defaultBaseUrl);

/// Max cached feed rows (read-cache LRU cap) + last-opened wiki pages.
const _feedCap = 200;
const _wikiCap = 48;

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  QCueText.registerFontLoaders();
  final prefs = await SharedPreferences.getInstance();

  // AU-R16: the running build number (pubspec `+BUILD`), surfaced to the update feature so it can
  // compare against the release manifest. Resolved once at bootstrap (the demo path shows it too).
  final currentBuild = int.tryParse((await PackageInfo.fromPlatform()).buildNumber) ?? 0;

  // Task 4: resolve the effective base URL at RUNTIME — a Settings override
  // (ServerUrlStore) beats the build-time `QCUE_BASE_URL` default, which beats
  // the local-bind fallback. So the app can be pointed at a deployed server
  // without a rebuild.
  final serverUrlStore = ServerUrlStore(prefs);
  final config = QcueConfig(
    baseUrl: QcueConfig.resolveBaseUrl(
      runtimeOverride: serverUrlStore.read(),
      buildTimeDefault: _baseUrl,
    ),
  );

  final overrides = <Override>[
    themeStoreProvider.overrideWithValue(SharedPrefsThemeStore(prefs)),
    qcueConfigProvider.overrideWithValue(config),
    serverUrlStoreProvider.overrideWithValue(serverUrlStore),
    // LOC-R2: the device-local "tag captures with location" toggle (off by
    // default), persisted in SharedPreferences. The in-memory default keeps
    // tests/stub keyless; this binds the durable store on the real-device path.
    captureLocationStoreProvider
        .overrideWithValue(SharedPrefsCaptureLocationStore(prefs)),
    // S4-R52: the durable "seen onboarding" flag. The keyless stub stays authed,
    // so it bypasses the onboarding gate regardless.
    onboardingStoreProvider.overrideWithValue(SharedPrefsOnboardingStore(prefs)),
    // AU-R16: the running build number for the update feature (shown in both demo + real builds).
    currentBuildProvider.overrideWithValue(currentBuild),
    // AU-R20: the device-local "automatic update check" toggle store (default on), in both modes.
    updatePrefsStoreProvider.overrideWithValue(SharedPrefsUpdatePrefsStore(prefs)),
  ];

  if (_useStub) {
    overrides
      ..add(apiClientProvider.overrideWithValue(StubApiClient.seeded()))
      // The demo build stays keyless: seed `authed` so the router bypasses login.
      ..add(authStateProvider.overrideWith(() => _AuthedStateNotifier()));
  } else {
    // Task 3: a DURABLE token store backed by the OS secure element so a
    // signed-in session survives a restart. Hydrated here (await) so accessSync
    // is correct from the first frame. A non-biometric store for the JWT pair
    // (the BYOK vault is the biometric-gated one) — we don't prompt every launch.
    const tokenSecureStore = NativeSecureStorage(requireBiometric: false);
    final tokens = await SecureTokenStore.load(tokenSecureStore);

    // SECURITY: origin-bind the durable session. If the effective server host changed since the session
    // was minted (the user re-pointed the Server URL at a different host, or a tampered `server_base_url`
    // pref), do NOT replay the existing JWT/refresh pair to a new origin — clear it and force a fresh
    // login. The bearer token (and any BYOK key the user later saves) must never be sent to a host the
    // session was not issued for. First run records the host without signing anyone out.
    final currentHost = Uri.tryParse(config.baseUrl)?.host ?? '';
    const sessionHostKey = 'qcue_session_host';
    final boundHost = prefs.getString(sessionHostKey);
    if (boundHost != null && boundHost != currentHost && tokens.accessSync.isNotEmpty) {
      await tokens.clear();
    }
    await prefs.setString(sessionHostKey, currentHost);

    // Real client behind the offline decorator + a durable sqlite read-cache.
    final dir = await getApplicationSupportDirectory();
    final cache = IdeaCache(
      SqliteCacheStore.open('${dir.path}/qcue_cache.db'),
      feedCap: _feedCap,
      wikiCap: _wikiCap,
    );
    // ISO-R2: if the device still holds a cache owned by a DIFFERENT account (the app was killed
    // between accounts, so no logout cleared it), wipe it now — before any provider reads it.
    final restoredSub = subjectOf(tokens.accessSync);
    if (restoredSub != null) cache.adoptOwner(restoredSub);
    final http.Client httpClient = http.Client();
    // Task 5/7: bound to the container below via a late forwarder.
    late final ProviderContainer container;
    final inner = HttpApiClient(
      config,
      tokens: tokens,
      httpClient: httpClient,
      // Task 5: refresh-on-401 — one rotation attempt, then sign out so the
      // router redirects to /login.
      onUnauthorized: () async {
        final ok = await container.read(authRepositoryProvider).refresh();
        if (!ok) {
          container.read(authStateProvider.notifier).markUnauthed();
        }
        return ok;
      },
    );
    final api = OfflineAwareApiClient(
      inner,
      cache,
      onSyncResult: (reason) {
        final notifier = container.read(syncStatusProvider.notifier);
        if (reason == null) {
          notifier.recordSuccess();
        } else {
          notifier.recordError(reason);
        }
      },
      // LOC-R1/R2: real action-time GPS, gated by the device-local toggle. The
      // closure reads the late-bound container lazily (same as onSyncResult), so
      // a capture only fetches a fix when the user has turned it on.
      locationService: const GeolocatorLocationService(),
      locationEnabled: () =>
          container.read(captureLocationStoreProvider).enabled,
    );
    // Sync Phase 1 (Task 12): the read-sync client over the same offline cache.
    // Register + pull on start/resume/online/periodic so a change made on (or by
    // the server for) another device surfaces in this device's feed/wiki after a
    // pull. Inert under the stub path (which has no cache / never builds this).
    final sync = SyncClient(
      registerDevice: api.registerDevice,
      pullSync: api.pullSync,
      cache: cache,
      platform: Platform.isIOS ? 'ios' : 'android',
    );
    overrides
      ..add(apiClientProvider.overrideWithValue(api))
      ..add(syncClientProvider.overrideWithValue(sync))
      ..add(tokenStoreProvider.overrideWithValue(tokens))
      // AU-R14: the real Shorebird status facade — only on a real app build (the demo keeps the stub;
      // a non-Shorebird build reports `isAvailable == false`, so this is benign there regardless).
      ..add(codePushFacadeProvider.overrideWithValue(ShorebirdCodePushFacade()))
      // AU-R21: the real APK install-intent channel (only invoked on Android's full-update path).
      ..add(installerChannelProvider.overrideWithValue(const MethodInstallerChannel()))
      // S5/D4: real device capabilities behind the existing seams. Voice capture
      // records an audio clip (RecordPackageRecorder) and transcribes it via the
      // cloud STT (/v1/transcribe → OpenAI gpt-4o-transcribe with the tenant's BYOK
      // key); the transcript lands in the editable compose field for review. The
      // device-cached BYOK key store (D9) is backed by the OS Keychain/Keystore,
      // gated by biometrics on read. (Native handlers are in MainActivity / AppDelegate.)
      ..add(voiceCaptureProvider.overrideWithValue(
        NativeVoiceCaptureController(
          recorder: RecordPackageRecorder(),
          // D4: record → upload → cloud STT (gpt-4o-transcribe); language auto-detected.
          transcribeCloud: ({required audio, language}) =>
              api.transcribe(audio: audio, language: language),
        ),
      ))
      ..add(secureStorageProvider.overrideWithValue(
        const NativeSecureStorage(requireBiometric: true),
      ))
      // A lightweight `/readyz` reachability probe drives connectivity. "The
      // radio is up" is not "the server is reachable" — this asks the real
      // question, and runs headlessly (unlike a platform-channel plugin).
      ..add(connectivitySourceProvider.overrideWithValue(
        PingConnectivitySource(
          ping: () async {
            final r = await httpClient
                .get(config.uri('/readyz'))
                .timeout(const Duration(seconds: 3));
            return r.statusCode;
          },
        ),
      ));

    container = ProviderContainer(overrides: overrides);
    // Register this device post-bootstrap (best-effort: offline / a signed-out
    // session just skips — the next trigger retries). The first pull fires from
    // the host's post-frame callback so it doesn't block the first frame.
    unawaited(() async {
      try {
        await sync.register();
      } catch (_) {/* offline / not signed in — a later trigger retries */}
    }());
    runApp(
      UncontrolledProviderScope(
        container: container,
        child: const ConnectivityHost(child: QCueApp()),
      ),
    );
    return;
  }

  runApp(
    ProviderScope(
      overrides: overrides,
      child: const ConnectivityHost(child: QCueApp()),
    ),
  );
}

/// Android headless background-flush entrypoint (S5-R37). The native FlushWorker
/// runs this via `executeDartEntrypoint`, which in an AOT build can only resolve
/// a function in the ROOT library (this file) by name — so this thin wrapper
/// lives here and delegates to the real implementation in `background/`.
@pragma('vm:entry-point')
Future<void> backgroundFlushMain() => bg_flush.backgroundFlushMain();

/// A [Notifier] seeded `authed` — used by the `QCUE_STUB` demo so the router
/// never gates the keyless demo behind a login screen.
class _AuthedStateNotifier extends AuthStateNotifier {
  @override
  AuthStatus build() => AuthStatus.authed;
}

/// Watches connectivity + app lifecycle and (a) flushes the outbound capture
/// queue and (b) pulls the read-sync change feed whenever the app comes back
/// online or resumes (S4-R26; Sync Phase 1 Task 12). Inert under the stub (the
/// seeded client has no IdeaCache and no [SyncClient]). Public so the sync-
/// trigger test can pump it directly with a spy SyncClient override.
class ConnectivityHost extends ConsumerStatefulWidget {
  const ConnectivityHost({super.key, required this.child});
  final Widget child;
  @override
  ConsumerState<ConnectivityHost> createState() => _ConnectivityHostState();
}

class _ConnectivityHostState extends ConsumerState<ConnectivityHost>
    with WidgetsBindingObserver {
  Timer? _poll;
  Timer? _syncPoll;
  ProactiveRefresh? _proactiveRefresh;

  // S5 native channels — live only on the real-device path (the OfflineAware
  // client); inert under the stub / in host tests (the fakes never construct
  // these). The capture seam binds to the offline-safe, idempotent capture; the
  // deep-link binds to the single GoRouter.
  ShareChannel? _share;
  WidgetChannel? _widget;
  NotifChannel? _notif;
  BackgroundFlush? _background;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    // Periodically re-probe reachability so the banner + flush track reality.
    _poll = Timer.periodic(const Duration(seconds: 10), (_) => _probe());
    WidgetsBinding.instance.addPostFrameCallback((_) => _probe());
    // Sync Phase 1 (Task 12): pull the read-sync change feed on first frame and
    // on a slower cadence than the connectivity probe (sync is heavier).
    WidgetsBinding.instance.addPostFrameCallback((_) => _syncPull());
    _syncPoll = Timer.periodic(const Duration(seconds: 30), (_) => _syncPull());
    // Flush + pull whenever we transition back online.
    ref.listenManual<Connectivity>(connectivityProvider, (prev, next) {
      if (next == Connectivity.online) {
        _flush();
        _syncPull();
      }
    });
    // AUTH-R5: schedule a proactive refresh at ~80% of the access TTL using the
    // persisted expires_at, then re-arm after every successful refresh. Only on
    // the real-device path (the stub stays authed; tokenStoreProvider is the
    // in-memory default there with no expiry).
    final tokens = ref.read(tokenStoreProvider);
    final repo = ref.read(authRepositoryProvider);
    _proactiveRefresh = ProactiveRefresh(refresh: () async {
      final ok = await repo.refresh();
      if (ok) {
        _proactiveRefresh?.schedule(ref.read(tokenStoreProvider).expiresAtSync);
      } else {
        ref.read(authStateProvider.notifier).markUnauthed();
      }
      return ok;
    });
    WidgetsBinding.instance
        .addPostFrameCallback((_) => _proactiveRefresh?.schedule(tokens.expiresAtSync));
    WidgetsBinding.instance.addPostFrameCallback((_) => _startNativeChannels());
  }

  /// S5: on the real-device path, start listening on the share + notification
  /// channels, refresh the widget, and schedule the background flush. Every
  /// native capture path enqueues through the SAME offline-safe idempotent
  /// capture (S5-R2/R11); deep-links route through the single GoRouter.
  void _startNativeChannels() {
    final api = ref.read(apiClientProvider);
    if (api is! OfflineAwareApiClient) return; // stub / tests: inert
    final router = ref.read(routerProvider);

    Future<void> enqueue(CaptureEnqueueReq req) =>
        api.capture(body: req.body, origin: req.origin);
    void deepLink(String route) => router.go(route);

    _share = ShareChannel(enqueue: enqueue)..start();
    unawaited(_share!.drainPending()); // pull Share-Extension-staged items

    _widget = WidgetChannel(enqueue: enqueue, onDeepLink: deepLink)..start();
    _notif = NotifChannel(onDeepLink: deepLink)..start();

    _background = BackgroundFlush(flush: api.flushOutbox);
    unawaited(_background!.schedulePeriodic());

    // Refresh the widget's non-sensitive count after the first feed read.
    unawaited(_refreshWidget());
  }

  Future<void> _refreshWidget() async {
    final api = ref.read(apiClientProvider);
    if (api is! OfflineAwareApiClient || _widget == null) return;
    try {
      // Count by the user's LOCAL calendar day (not a rolling 24h window — see todaysLocalCount).
      final today = todaysLocalCount(api.cache.feed(), DateTime.now());
      await _widget!.refresh(todayCount: today);
    } catch (_) {/* widget refresh is best-effort */}
  }

  Future<void> _probe() async {
    if (!mounted) return;
    await ref.read(connectivityProvider.notifier).probe();
  }

  Future<void> _flush() async {
    final api = ref.read(apiClientProvider);
    if (api is OfflineAwareApiClient) await api.flushOutbox();
  }

  /// Sync Phase 1 (Task 12): pull the read-sync change feed into the offline
  /// cache. Inert when no [SyncClient] is wired (stub / demo). Best-effort — a
  /// network error is swallowed; the next trigger retries.
  Future<void> _syncPull() async {
    final sync = ref.read(syncClientProvider);
    if (sync == null) return;
    try {
      // Bump the cache revision ONLY when the pull actually changed the cache, so
      // the read-providers re-read (surfacing digest/recall results + another
      // device's captures) without a relaunch — and a no-op pull costs no re-fetch.
      final changed = await sync.pull();
      if (changed && mounted) ref.read(cacheRevisionProvider.notifier).bump();
    } catch (_) {/* offline / not signed in — a later trigger retries */}
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      _probe();
      _flush();
      _syncPull(); // Sync Phase 1 (Task 12): pull on resume
      // S5: drain any items the Share Extension staged while we were away +
      // refresh the widget count.
      if (_share != null) unawaited(_share!.drainPending());
      unawaited(_refreshWidget());
    }
  }

  @override
  void dispose() {
    _poll?.cancel();
    _syncPoll?.cancel();
    _proactiveRefresh?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    unawaited(_share?.dispose());
    unawaited(_widget?.dispose());
    unawaited(_notif?.dispose());
    _background?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
