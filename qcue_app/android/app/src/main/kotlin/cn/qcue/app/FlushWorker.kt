package cn.qcue.app

import android.content.Context
import androidx.work.Worker
import androidx.work.WorkerParameters

/**
 * QCue S5-R37/R38 — the WorkManager job that wakes the app to drain the offline
 * outbound capture queue when the OS grants a background window + the network is
 * up. The actual drain is the Rust/Dart `sync_flush_now` reached by re-entering
 * a headless Flutter engine; this worker only triggers it and reports the OS
 * result. It is idempotent (the queue dedupes on client id, S5-R38), so a retried
 * run never double-POSTs.
 *
 * NO business logic lives here (S5-R1). NOTE (native-unverified-here): code-complete
 * for the Android toolchain; NOT compiled/run on the Linux CI host.
 */
class FlushWorker(
    context: Context,
    params: WorkerParameters,
) : Worker(context, params) {

    override fun doWork(): Result {
        return try {
            // Start a headless FlutterEngine, run backgroundFlushMain (rebuilds the
            // offline client + drains the queue), and block until it signals done.
            // The drain is idempotent (S5-R38), so a retried run never double-POSTs.
            val ok = BackgroundPlugin.triggerHeadlessFlush(applicationContext)
            if (ok) Result.success() else Result.retry() // timed out / engine failed
        } catch (_: Exception) {
            Result.retry() // S5-R41 — never crash; the next window retries.
        }
    }

    companion object {
        const val UNIQUE_WORK = "qcue.flush.periodic"
    }
}
