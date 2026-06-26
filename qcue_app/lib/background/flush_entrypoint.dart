// QCue S5-R37/R38/R41 — the Android headless background-flush entrypoint.
//
// WorkManager's FlushWorker runs in the app process but WITHOUT the foreground
// FlutterEngine/isolate, so the native side starts a fresh headless engine and
// executes THIS top-level entrypoint (by name) to actually drain the offline
// outbound capture queue. It rebuilds a minimal offline client over the SAME
// SQLite file + token store as `main()`, flushes once (idempotent — the queue
// dedupes on client uuidv7, S5-R38), signals the worker, and exits. All errors
// are swallowed so a dead network never crashes the worker (S5-R41).
//
// iOS does NOT use this — its BGTask relaunches the app and reuses the implicit
// engine, reaching the Dart `runFlush` handler on the main channel instead.
import 'dart:ui' show DartPluginRegistrant;

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:http/http.dart' as http;
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../core/native/secure/native_secure_storage.dart';
import '../core/net/http_api_client.dart';
import '../core/net/qcue_config.dart';
import '../core/net/secure_token_store.dart';
import '../core/net/server_url_store.dart';
import '../core/offline/idea_cache.dart';
import '../core/offline/offline_api_client.dart';
import '../core/offline/sqlite_cache_store.dart';

/// Build-time base URL (mirrors `main.dart`); the runtime Settings override wins.
const _baseUrl = String.fromEnvironment('QCUE_BASE_URL',
    defaultValue: QcueConfig.defaultBaseUrl);

/// Read-cache caps — kept in sync with `main.dart` (same on-disk cache).
const _feedCap = 200;
const _wikiCap = 48;

/// The signal channel back to the native [FlushWorker] (`flushDone`).
const _headless = MethodChannel('qcue/background/headless');

/// Headless background flush. Registered as a Dart entrypoint and invoked by the
/// native FlushWorker via a fresh FlutterEngine. NOT called from app code.
@pragma('vm:entry-point')
Future<void> backgroundFlushMain() async {
  WidgetsFlutterBinding.ensureInitialized();
  // Background isolates must register plugins explicitly (path_provider /
  // shared_preferences / sqlite3 are used below).
  DartPluginRegistrant.ensureInitialized();

  http.Client? httpClient;
  try {
    final prefs = await SharedPreferences.getInstance();
    final config = QcueConfig(
      baseUrl: QcueConfig.resolveBaseUrl(
        runtimeOverride: ServerUrlStore(prefs).read(),
        buildTimeDefault: _baseUrl,
      ),
    );

    // Same secure token store + on-disk SQLite cache as the foreground app, so
    // we drain the exact same outbound queue.
    const tokenSecureStore = NativeSecureStorage(requireBiometric: false);
    final tokens = await SecureTokenStore.load(tokenSecureStore);
    final dir = await getApplicationSupportDirectory();
    final cache = IdeaCache(
      SqliteCacheStore.open('${dir.path}/qcue_cache.db'),
      feedCap: _feedCap,
      wikiCap: _wikiCap,
    );
    httpClient = http.Client();
    final api = OfflineAwareApiClient(
      HttpApiClient(config, tokens: tokens, httpClient: httpClient),
      cache,
    );

    await api.flushOutbox(); // idempotent drain (S5-R38)
  } catch (_) {
    // S5-R41: offline / not signed in / server down — leave the queue for the
    // next window; never crash the worker.
  } finally {
    httpClient?.close();
    // Tell the worker we're done so it can finish + tear down the engine.
    try {
      await _headless.invokeMethod<void>('flushDone');
    } catch (_) {/* worker already gone */}
  }
}
