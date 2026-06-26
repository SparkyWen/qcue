package cn.qcue.app

import android.Manifest
import android.app.Activity
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.ActivityCompat
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/**
 * QCue S5-R33/R34 — the thin Android local-notification bridge.
 *
 * Shows a notification for the three closed [QNotifKind]s (dreamComplete,
 * ingestNeedsReview, syncConflict) over `qcue/notif`; a tap fires a deep-link
 * PendingIntent that re-enters the MainActivity carrying the route, which the
 * MainActivity forwards over `qcue/notif/events` for go_router (S5-R34). The
 * route map is opaque to native — Dart owns navigation. Push/FCM is roadmap;
 * `registerPushToken` is a documented no-op stub here.
 *
 * NO business logic lives here (S5-R1). NOTE (native-unverified-here): code-complete
 * for the Android toolchain; NOT compiled or unit-run on the Linux CI host. A
 * Robolectric test accompanies it in src/test.
 */
class NotifPlugin(
    private val context: Context,
    private val method: MethodChannel,
    private val events: EventChannel,
    /**
     * The host Activity, needed to launch the Android 13+ POST_NOTIFICATIONS
     * prompt (S5-R30). Null in unit tests → `requestPermission` reports the passive
     * channel-enabled status.
     */
    private val activity: Activity? = null,
) : MethodChannel.MethodCallHandler, EventChannel.StreamHandler {

    private var sink: EventChannel.EventSink? = null
    private var pendingPermission: MethodChannel.Result? = null

    fun attach() {
        method.setMethodCallHandler(this)
        events.setStreamHandler(this)
        ensureChannels()
    }

    fun detach() {
        method.setMethodCallHandler(null)
        events.setStreamHandler(null)
    }

    override fun onListen(arguments: Any?, sink: EventChannel.EventSink?) {
        this.sink = sink
    }

    override fun onCancel(arguments: Any?) {
        sink = null
    }

    /** Re-deliver a tap route forwarded from MainActivity.onNewIntent. */
    fun deliverTap(kind: String, route: Map<String, String>) {
        sink?.success(mapOf("kind" to kind, "route" to route))
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        if ((call.argument<Int>("schemaVersion") ?: 0) != QcueChannels.SCHEMA_VERSION) {
            result.error("versionMismatch", "unsupported schemaVersion",
                mapOf("kind" to "versionMismatch", "retryable" to false))
            return
        }
        try {
            when (call.method) {
                "requestPermission" -> requestPermission(result)
                "show" -> { show(call); result.success(null) }
                "cancelKind" -> {
                    cancel(call.argument<String>("kind"))
                    result.success(null)
                }
                "registerPushToken" -> result.success(null) // roadmap stub (S5-R35)
                else -> result.notImplemented()
            }
        } catch (e: Exception) {
            result.error("osError", e.message, mapOf("kind" to "osError", "retryable" to false))
        }
    }

    private fun permissionStatus(): String {
        // POST_NOTIFICATIONS is runtime-requested on Android 13+; the Activity
        // performs the request. Here we report whether notifications are enabled.
        val nm = nm()
        return if (nm.areNotificationsEnabled()) "granted" else "denied"
    }

    /**
     * S5-R30 — actually launch the Android 13+ POST_NOTIFICATIONS prompt. The
     * previous impl only read areNotificationsEnabled(), so on API 33+ the runtime
     * grant was never requested and every notification (dream / review / conflict)
     * was silently dropped. Pre-13 has no runtime notif permission, so we report the
     * channel-enabled status. The result is held until [onRequestPermissionsResult];
     * with no host Activity (tests) or a request in flight, falls back to passive.
     */
    private fun requestPermission(result: MethodChannel.Result) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
            result.success(permissionStatus())
            return
        }
        val granted = ContextCompat.checkSelfPermission(
            context, Manifest.permission.POST_NOTIFICATIONS,
        ) == PackageManager.PERMISSION_GRANTED
        if (granted) {
            result.success("granted")
            return
        }
        val act = activity
        if (act == null || pendingPermission != null) {
            result.success(permissionStatus())
            return
        }
        pendingPermission = result
        try {
            ActivityCompat.requestPermissions(
                act, arrayOf(Manifest.permission.POST_NOTIFICATIONS), POST_NOTIF_REQUEST,
            )
        } catch (e: Exception) {
            pendingPermission = null
            result.success(permissionStatus())
        }
    }

    /** Completes the in-flight [requestPermission] from MainActivity's callback. */
    fun onRequestPermissionsResult(requestCode: Int, grantResults: IntArray): Boolean {
        if (requestCode != POST_NOTIF_REQUEST) return false
        val granted = grantResults.isNotEmpty() &&
            grantResults[0] == PackageManager.PERMISSION_GRANTED
        pendingPermission?.success(if (granted) "granted" else "denied")
        pendingPermission = null
        return true
    }

    private fun show(call: MethodCall) {
        val kind = call.argument<String>("kind") ?: return
        val title = call.argument<String>("title") ?: "QCue"
        val body = call.argument<String>("body") ?: ""
        @Suppress("UNCHECKED_CAST")
        val route = (call.argument<Map<String, Any?>>("route") ?: emptyMap())
            .mapValues { it.value?.toString() ?: "" }

        // Tap → re-enter MainActivity with the route extras (S5-R34).
        val intent = Intent(context, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            action = ACTION_TAP
            putExtra(EXTRA_KIND, kind)
            route.forEach { (k, v) -> putExtra("route_$k", v) }
        }
        val pending = PendingIntent.getActivity(
            context, kind.hashCode(), intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val n = NotificationCompat.Builder(context, channelFor(kind))
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle(title)
            .setContentText(body)
            .setAutoCancel(true)
            .setContentIntent(pending)
            .build()
        nm().notify(kind.hashCode(), n) // stable id per kind → tap idempotent
    }

    private fun cancel(kind: String?) {
        if (kind != null) nm().cancel(kind.hashCode())
    }

    private fun ensureChannels() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = nm()
        // Human-readable channel names: these surface verbatim in the system
        // Settings > Notifications UI, so the raw kind tokens (dreamComplete/…)
        // would otherwise leak there. IMPORTANCE_DEFAULT is audible, matching the
        // iOS .sound authorization/presentation. (iOS has no user-facing channel
        // label, so this is Android-only polish toward the same UX.)
        val names = mapOf(
            "dreamComplete" to "Dream complete",
            "ingestNeedsReview" to "Captures need review",
            "syncConflict" to "Sync conflicts",
        )
        for ((k, label) in names) {
            nm.createNotificationChannel(
                NotificationChannel(channelFor(k), label, NotificationManager.IMPORTANCE_DEFAULT),
            )
        }
    }

    private fun channelFor(kind: String) = "qcue.$kind"
    private fun nm() = androidx.core.app.NotificationManagerCompat.from(context)

    companion object {
        const val ACTION_TAP = "cn.qcue.NOTIF_TAP"
        const val EXTRA_KIND = "kind"
        const val POST_NOTIF_REQUEST = 0x517F
    }
}
