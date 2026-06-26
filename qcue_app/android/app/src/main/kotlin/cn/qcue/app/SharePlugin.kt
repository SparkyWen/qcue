package cn.qcue.app

import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import androidx.core.content.edit
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.json.JSONArray
import org.json.JSONObject

/**
 * QCue S5-R42/R43/R44 — the thin Android share-sheet bridge.
 *
 * The MainActivity declares an `ACTION_SEND` (text/plain, text/uri-list) intent
 * filter; the shared item is forwarded here. Because the app may be cold-started
 * by the share, items are STAGED in a small SharedPreferences-backed store (the
 * Android analog of the iOS App Group container) and the Dart side drains them on
 * launch/resume via `drainPending`; when the app is already running, the item is
 * also emitted live over `qcue/share/events`.
 *
 * NO business logic lives here (S5-R1): the plugin stages/forwards the verbatim
 * blob + its source; capture persistence, dedup and the S2 untrusted-fencing all
 * happen downstream in the shared Rust core / offline queue. Size + type caps are
 * enforced at the boundary (S5-R44); oversize/unknown types are dropped.
 *
 * NOTE (native-unverified-here): code-complete for the Android toolchain; NOT
 * compiled or unit-run on the Linux CI host (no Android SDK). A Robolectric test
 * accompanies it in src/test.
 */
class SharePlugin(
    private val context: Context,
    private val method: MethodChannel,
    private val events: EventChannel,
) : MethodChannel.MethodCallHandler, EventChannel.StreamHandler {

    private val main = Handler(Looper.getMainLooper())
    private var sink: EventChannel.EventSink? = null

    fun attach() {
        method.setMethodCallHandler(this)
        events.setStreamHandler(this)
    }

    fun detach() {
        method.setMethodCallHandler(null)
        events.setStreamHandler(null)
    }

    // ── MethodChannel ────────────────────────────────────────────────────────

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        if ((call.argument<Int>("schemaVersion") ?: 0) != QcueChannels.SCHEMA_VERSION) {
            result.error("versionMismatch", "unsupported schemaVersion", versionDetails())
            return
        }
        when (call.method) {
            "drainPending" -> result.success(drainStaged())
            else -> result.notImplemented()
        }
    }

    // ── EventChannel ─────────────────────────────────────────────────────────

    override fun onListen(arguments: Any?, sink: EventChannel.EventSink?) {
        this.sink = sink
    }

    override fun onCancel(arguments: Any?) {
        sink = null
    }

    // ── Intent handling (called from MainActivity.onNewIntent / onCreate) ─────

    /**
     * Parse an inbound ACTION_SEND intent into a shared-item map and deliver it
     * EXACTLY ONCE. When a live Dart listener is attached (app already running) the
     * item is emitted live and NOT staged; otherwise (cold-started by the share) it
     * is staged for `drainPending` on launch. Staging AND live-emitting the same
     * item would re-drain it on the next resume and double-capture it (the offline
     * queue mints a fresh client id per enqueue, so it cannot dedupe the copy) —
     * this mirrors iOS, whose Share Extension only ever stages and whose every read
     * drains-and-clears. Returns the parsed map (or null).
     */
    fun handleSendIntent(intent: Intent?): Map<String, Any?>? {
        val item = parse(intent) ?: return null
        val liveSink = sink
        if (liveSink != null) {
            main.post { liveSink.success(item) } // running → deliver live, once
        } else {
            stage(item) // cold start → stage for drainPending on launch
        }
        return item
    }

    /** S5-R44 — accept text/plain + URLs only, cap text length; drop the rest. */
    private fun parse(intent: Intent?): Map<String, Any?>? {
        if (intent?.action != Intent.ACTION_SEND) return null
        // Mirror iOS's fixed "ios-share" provenance label. Intent.getPackage() is
        // almost always null for ACTION_SEND, so the real sender is unreliable; a
        // stable platform label keeps the capture origin symmetric with iOS
        // ('share:web:android-share' vs 'share:web:ios-share').
        val source = "android-share"
        val text = intent.getStringExtra(Intent.EXTRA_TEXT) ?: return null
        if (text.length > MAX_TEXT_CHARS) return null // oversize → drop (S5-R44)
        val looksUrl = text.startsWith("http://") || text.startsWith("https://")
        return buildMap {
            if (looksUrl) put("url", text) else put("text", text)
            put("sourceApp", source)
        }
    }

    // ── staging store (SharedPreferences ≈ the iOS App Group container) ────────

    private fun prefs() = context.getSharedPreferences(STAGE, Context.MODE_PRIVATE)

    private fun stage(item: Map<String, Any?>) {
        val arr = JSONArray(prefs().getString(KEY, "[]"))
        arr.put(JSONObject(item.filterValues { it != null }))
        prefs().edit { putString(KEY, arr.toString()) }
    }

    private fun drainStaged(): List<Map<String, Any?>> {
        val arr = JSONArray(prefs().getString(KEY, "[]"))
        val out = ArrayList<Map<String, Any?>>(arr.length())
        for (i in 0 until arr.length()) {
            val o = arr.getJSONObject(i)
            out.add(o.keys().asSequence().associateWith { o.get(it) })
        }
        prefs().edit { remove(KEY) } // drained exactly once
        return out
    }

    private fun versionDetails() = mapOf("kind" to "versionMismatch", "retryable" to false)

    companion object {
        private const val STAGE = "qcue.share.stage"
        private const val KEY = "pending"
        private const val MAX_TEXT_CHARS = 100_000
    }
}
