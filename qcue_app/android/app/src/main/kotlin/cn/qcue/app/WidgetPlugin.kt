package cn.qcue.app

import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Context
import androidx.core.content.edit
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/**
 * QCue S5-R46/R47 — the thin Android home-screen-widget bridge.
 *
 * `setCount` writes the NON-SENSITIVE today-capture count into the shared prefs
 * the widget reads (S5-R46 — never idea/wiki bodies on the home screen);
 * `reloadTimelines` asks AppWidgetManager to refresh every [QuickCaptureWidget]
 * instance (S5-R47). Widget-tap intents (compose / quickCapture) are forwarded
 * over `qcue/widget/events` from the MainActivity / the broadcast receiver.
 *
 * NO business logic lives here (S5-R1). NOTE (native-unverified-here): code-complete
 * for the Android toolchain; NOT compiled or unit-run on the Linux CI host. A
 * Robolectric test accompanies it in src/test.
 */
class WidgetPlugin(
    private val context: Context,
    private val method: MethodChannel,
    private val events: EventChannel,
) : MethodChannel.MethodCallHandler, EventChannel.StreamHandler {

    private var sink: EventChannel.EventSink? = null

    fun attach() {
        method.setMethodCallHandler(this)
        events.setStreamHandler(this)
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

    /** Forward a widget tap (compose / quickCapture) to Dart (S5-R45). */
    fun deliverTap(action: String, args: Map<String, Any?> = emptyMap()) {
        sink?.success(mapOf("action" to action, "args" to args))
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        if ((call.argument<Int>("schemaVersion") ?: 0) != QcueChannels.SCHEMA_VERSION) {
            result.error("versionMismatch", "unsupported schemaVersion",
                mapOf("kind" to "versionMismatch", "retryable" to false))
            return
        }
        when (call.method) {
            "setCount" -> {
                val count = call.argument<Int>("count") ?: 0
                context.getSharedPreferences(STORE, Context.MODE_PRIVATE)
                    .edit { putInt(KEY_COUNT, count) }
                result.success(null)
            }
            "reloadTimelines" -> {
                reload()
                result.success(null)
            }
            else -> result.notImplemented()
        }
    }

    private fun reload() {
        val mgr = AppWidgetManager.getInstance(context)
        val ids = mgr.getAppWidgetIds(ComponentName(context, QuickCaptureWidget::class.java))
        if (ids.isNotEmpty()) {
            QuickCaptureWidget().onUpdate(context, mgr, ids)
        }
    }

    companion object {
        const val STORE = "qcue.widget"
        const val KEY_COUNT = "todayCount"
    }
}
