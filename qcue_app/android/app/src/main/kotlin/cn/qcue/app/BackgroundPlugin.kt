package cn.qcue.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import io.flutter.FlutterInjector
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.dart.DartExecutor
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * QCue S5-R37/R38 — the thin Android background-flush scheduler bridge over
 * `qcue/background`. `schedulePeriodic` enqueues a UNIQUE, network-constrained
 * periodic [FlushWorker] (REPLACE keeps it singular → idempotent scheduling);
 * `cancel` removes it. The drain is the offline queue's idempotent flush.
 *
 * NO business logic lives here (S5-R1). NOTE (native-unverified-here): code-complete
 * for the Android toolchain; NOT compiled/run on the Linux CI host. A Robolectric
 * test accompanies it in src/test.
 */
class BackgroundPlugin(
    private val context: Context,
    private val method: MethodChannel,
) : MethodChannel.MethodCallHandler {

    fun attach() = method.setMethodCallHandler(this)
    fun detach() = method.setMethodCallHandler(null)

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        if ((call.argument<Int>("schemaVersion") ?: 0) != QcueChannels.SCHEMA_VERSION) {
            result.error("versionMismatch", "unsupported schemaVersion",
                mapOf("kind" to "versionMismatch", "retryable" to false))
            return
        }
        when (call.method) {
            "schedulePeriodic" -> {
                val requiresNetwork = call.argument<Boolean>("requiresNetwork") ?: true
                schedule(requiresNetwork)
                result.success(null)
            }
            "cancel" -> {
                WorkManager.getInstance(context).cancelUniqueWork(FlushWorker.UNIQUE_WORK)
                result.success(null)
            }
            else -> result.notImplemented()
        }
    }

    private fun schedule(requiresNetwork: Boolean) {
        val constraints = Constraints.Builder()
            .setRequiredNetworkType(
                if (requiresNetwork) NetworkType.CONNECTED else NetworkType.NOT_REQUIRED,
            )
            .build()
        // Min WorkManager period is 15 minutes; the queue also flushes on resume
        // + on reachability, so this is the safety net (S5-R37).
        val work = PeriodicWorkRequestBuilder<FlushWorker>(15, TimeUnit.MINUTES)
            .setConstraints(constraints)
            .build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(
            FlushWorker.UNIQUE_WORK,
            ExistingPeriodicWorkPolicy.UPDATE, // REPLACE-like → single unique item
            work,
        )
    }

    companion object {
        /**
         * Entrypoint the headless engine runs. It MUST be a top-level function in
         * the ROOT library (lib/main.dart) — in an AOT release build the engine can
         * only resolve a name-only entrypoint there (see the main.dart wrapper).
         */
        private const val HEADLESS_ENTRYPOINT = "backgroundFlushMain"
        private const val HEADLESS_CHANNEL = "qcue/background/headless"
        private const val HEADLESS_TIMEOUT_SEC = 120L

        /**
         * Run the headless Dart flush from [FlushWorker] (S5-R37/R38). WorkManager
         * runs without the app's FlutterEngine, so we start a fresh one, register
         * the plugins, execute [backgroundFlushMain] (rebuilds the offline client +
         * drains the queue), and block until it signals `flushDone`. The engine is
         * created/destroyed on the main thread; the worker thread waits on a latch.
         * Returns true if the drain ran to completion (→ Result.success).
         *
         * NOTE (native-unverified-here): exercised on-device/emulator by force-running
         * the WorkManager job; not reachable from the Robolectric unit tests.
         */
        fun triggerHeadlessFlush(context: Context): Boolean {
            val app = context.applicationContext
            val latch = CountDownLatch(1)
            val ok = AtomicBoolean(false)
            Handler(Looper.getMainLooper()).post {
                var engine: FlutterEngine? = null
                try {
                    val loader = FlutterInjector.instance().flutterLoader()
                    loader.startInitialization(app)
                    loader.ensureInitializationComplete(app, null)
                    engine = FlutterEngine(app)
                    // Register plugins so path_provider / shared_preferences / sqlite3
                    // work inside the headless isolate.
                    io.flutter.plugins.GeneratedPluginRegistrant.registerWith(engine)
                    // The headless isolate also needs the secure-store channel to read
                    // the auth tokens (SecureTokenStore). The token store is
                    // requireBiometric:false, so no Activity/biometric is involved; the
                    // gate denies (fail closed) for the unused biometric path.
                    SecurePlugin(
                        app,
                        MethodChannel(engine.dartExecutor.binaryMessenger, QcueChannels.SECURE),
                        biometricGate = { _, cb -> cb(BiometricOutcome.DENIED) },
                    ).attach()
                    MethodChannel(engine.dartExecutor.binaryMessenger, HEADLESS_CHANNEL)
                        .setMethodCallHandler { call, result ->
                            if (call.method == "flushDone") {
                                ok.set(true)
                                result.success(null)
                                engine?.destroy()
                                latch.countDown()
                            } else {
                                result.notImplemented()
                            }
                        }
                    engine.dartExecutor.executeDartEntrypoint(
                        DartExecutor.DartEntrypoint(
                            loader.findAppBundlePath(),
                            HEADLESS_ENTRYPOINT,
                        ),
                    )
                } catch (e: Exception) {
                    engine?.destroy()
                    latch.countDown()
                }
            }
            // Block the worker thread until Dart signals done (or the safety cap).
            latch.await(HEADLESS_TIMEOUT_SEC, TimeUnit.SECONDS)
            return ok.get()
        }
    }
}
