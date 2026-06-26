package cn.qcue.app

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.speech.RecognitionListener
import android.speech.RecognitionSupport
import android.speech.RecognitionSupportCallback
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.content.Intent
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.util.Locale

/**
 * QCue S5-R18/R19/R21 — the thin Android on-device STT bridge.
 *
 * Marshals the OS [SpeechRecognizer] across `qcue/stt` (method) + `qcue/stt/events`
 * (event). It owns the mic itself (the recognizer captures audio); the §6 audio
 * recorder is a separate escalation path. NO business logic lives here — escalation
 * policy, capture persistence and dedup are all in the shared Rust core (S5-R1).
 *
 * NOTE (native-unverified-here): code-complete for the Android toolchain; it is NOT
 * compiled or unit-run on the Linux CI host (no Android SDK). JUnit/Robolectric
 * tests accompany it in src/test.
 */
class SttPlugin(
    private val context: Context,
    private val method: MethodChannel,
    private val events: EventChannel,
    /**
     * The host Activity, needed to launch the runtime RECORD_AUDIO prompt
     * (S5-R18/R30). Null in unit tests → `requestPermission` reports passive status.
     */
    private val activity: Activity? = null,
) : MethodChannel.MethodCallHandler, EventChannel.StreamHandler {

    private val main = Handler(Looper.getMainLooper())
    private var recognizer: SpeechRecognizer? = null
    private var sink: EventChannel.EventSink? = null
    private var captureId: String? = null
    private var pendingPermission: MethodChannel.Result? = null

    fun attach() {
        method.setMethodCallHandler(this)
        events.setStreamHandler(this)
    }

    fun detach() {
        method.setMethodCallHandler(null)
        events.setStreamHandler(null)
        main.post { teardownRecognizer() }
    }

    // ── MethodChannel ──────────────────────────────────────────────────────

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        // S5-R3: reject an unknown major schema version rather than mis-parse.
        val version = call.argument<Int>("schemaVersion") ?: 0
        if (version != QcueChannels.SCHEMA_VERSION) {
            result.error(
                "versionMismatch",
                "unsupported schemaVersion $version",
                mapOf("kind" to "versionMismatch", "retryable" to false),
            )
            return
        }
        try {
            when (call.method) {
                "isAvailable" -> resolveAvailability(call.argument<String>("localeTag"), result)
                "requestPermission" -> requestPermission(result)
                "start" -> {
                    val locale = call.argument<String>("localeTag")
                    val cid = call.argument<String>("captureId") ?: ""
                    val partial = call.argument<Boolean>("partialResults") ?: true
                    start(cid, locale, partial)
                    result.success(null)
                }
                "stop" -> {
                    main.post { recognizer?.stopListening() }
                    result.success(null)
                }
                "cancel" -> {
                    main.post { recognizer?.cancel(); teardownRecognizer() }
                    result.success(null)
                }
                else -> result.notImplemented()
            }
        } catch (e: Exception) {
            // S5-R4: never leak a raw OS exception across the boundary.
            result.error("osError", e.message, mapOf("kind" to "osError", "retryable" to false))
        }
    }

    private fun isAvailable(): Boolean = SpeechRecognizer.isRecognitionAvailable(context)

    /**
     * Locale-aware availability, mirroring iOS `SFSpeechRecognizer(locale:).isAvailable`.
     * No locale → the device-wide probe (current behavior). With a locale on API 33+,
     * check it against the recognizer's supported set; below 33 there is no per-locale
     * API, so report the device-wide bool (the start() → ERROR_LANGUAGE_* path still
     * surfaces an unsupported locale). ALWAYS completes the result exactly once so a
     * caller can never hang (defense-in-depth alongside the Dart-side availTimeout).
     */
    private fun resolveAvailability(localeTag: String?, result: MethodChannel.Result) {
        val deviceHas = isAvailable()
        if (!deviceHas || localeTag == null) {
            result.success(deviceHas)
            return
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
            result.success(true)
            return
        }
        querySupportedLocales(
            onResult = { langs -> result.success(langs.any { localeMatches(it, localeTag) }) },
            onUnknown = { result.success(deviceHas) },
        )
    }

    /**
     * Enumerate the recognizer's supported locales via the API-33 RecognitionSupport
     * callback (mirrors iOS `SFSpeechRecognizer.supportedLocales()`), on the main
     * thread, completing exactly once. Caller handles the API-<33 / error fallback.
     */
    private fun querySupportedLocales(
        onResult: (List<String>) -> Unit,
        onUnknown: () -> Unit,
    ) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
            onUnknown(); return
        }
        main.post {
            val sr = try {
                SpeechRecognizer.createSpeechRecognizer(context)
            } catch (e: Exception) {
                onUnknown(); return@post
            }
            var done = false
            fun finish(langs: List<String>?) {
                if (done) return
                done = true
                sr.destroy()
                if (langs != null) onResult(langs) else onUnknown()
            }
            val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
                putExtra(
                    RecognizerIntent.EXTRA_LANGUAGE_MODEL,
                    RecognizerIntent.LANGUAGE_MODEL_FREE_FORM,
                )
            }
            try {
                sr.checkRecognitionSupport(
                    intent,
                    context.mainExecutor,
                    object : RecognitionSupportCallback {
                        override fun onSupportResult(support: RecognitionSupport) =
                            finish(
                                // On-device (supported + installed) ∪ cloud (online) —
                                // the full set, matching iOS supportedLocales().
                                (support.supportedOnDeviceLanguages +
                                    support.installedOnDeviceLanguages +
                                    support.onlineLanguages).distinct(),
                            )

                        override fun onError(error: Int) = finish(null)
                    },
                )
            } catch (e: Exception) {
                finish(null)
            }
        }
    }

    /** Maps the RECORD_AUDIO grant to the QPermStatus token Dart expects (S5-R18). */
    private fun permissionGranted(): Boolean = ContextCompat.checkSelfPermission(
        context, Manifest.permission.RECORD_AUDIO,
    ) == PackageManager.PERMISSION_GRANTED

    private fun permissionStatus(): String = if (permissionGranted()) "granted" else "denied"

    /**
     * S5-R18/R30 — actually launch the OS RECORD_AUDIO prompt. The previous impl
     * only read the passive status, so the dialog never showed and the first voice
     * capture always failed the gate (the mic returned an empty transcript). The
     * MethodChannel result is held until [onRequestPermissionsResult]. With no host
     * Activity (unit tests) or a request already in flight, falls back to the
     * passive status so the caller still gets a deterministic answer.
     */
    private fun requestPermission(result: MethodChannel.Result) {
        if (permissionGranted()) {
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
                act, arrayOf(Manifest.permission.RECORD_AUDIO), RECORD_AUDIO_REQUEST,
            )
        } catch (e: Exception) {
            pendingPermission = null
            result.success(permissionStatus())
        }
    }

    /**
     * Completes the in-flight [requestPermission] from MainActivity's
     * onRequestPermissionsResult. Returns true if this plugin owned the request.
     */
    fun onRequestPermissionsResult(requestCode: Int, grantResults: IntArray): Boolean {
        if (requestCode != RECORD_AUDIO_REQUEST) return false
        val granted = grantResults.isNotEmpty() &&
            grantResults[0] == PackageManager.PERMISSION_GRANTED
        pendingPermission?.success(if (granted) "granted" else "denied")
        pendingPermission = null
        return true
    }

    private fun start(cid: String, localeTag: String?, partial: Boolean) {
        captureId = cid
        // The locale actually used for recognition: the caller's tag, else the
        // device default. Reported back verbatim in the final event so it agrees
        // with what the recognizer was told (mirrors iOS, which reports the
        // resolved recognition locale, not a hardcoded device default).
        val resolvedLocale = localeTag ?: Locale.getDefault().toLanguageTag()
        main.post {
            if (!isAvailable()) {
                Log.w(TAG, "start: no recognizer on device (isRecognitionAvailable=false)")
                emitError(cid, "unavailable", "no recognizer on device")
                return@post
            }
            teardownRecognizer()
            val sr = SpeechRecognizer.createSpeechRecognizer(context)
            sr.setRecognitionListener(listener(cid, resolvedLocale))
            recognizer = sr
            // v0.2.2 mic fix: only FORCE offline recognition (D4) when the device genuinely has an
            // on-device recognizer. Forcing EXTRA_PREFER_OFFLINE=true on a phone without the offline
            // language pack made SpeechRecognizer fire an immediate error (ERROR_LANGUAGE_UNAVAILABLE /
            // ERROR_NO_MATCH / ERROR_CLIENT) — the "mic flips active→inactive, records nothing" bug.
            // When offline isn't genuinely available we omit the extra so the system uses its default
            // (online-capable) recognizer instead of failing fast.
            val preferOffline = shouldPreferOffline(onDeviceRecognitionAvailable())
            Log.i(
                TAG,
                "start: cid=$cid locale=$resolvedLocale partial=$partial preferOffline=$preferOffline",
            )
            val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
                putExtra(
                    RecognizerIntent.EXTRA_LANGUAGE_MODEL,
                    RecognizerIntent.LANGUAGE_MODEL_FREE_FORM,
                )
                putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, partial)
                putExtra(RecognizerIntent.EXTRA_LANGUAGE, resolvedLocale)
                if (preferOffline) {
                    putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
                }
            }
            sr.startListening(intent)
        }
    }

    private fun teardownRecognizer() {
        recognizer?.destroy()
        recognizer = null
    }

    // ── EventChannel ───────────────────────────────────────────────────────

    override fun onListen(arguments: Any?, sink: EventChannel.EventSink?) {
        this.sink = sink
        // Announce availability on subscribe (S5-R3 avail event). onDeviceAvailable
        // must mean genuine OFFLINE recognition (D4) to match iOS's
        // supportsOnDeviceRecognition — not "any recognizer exists" (which is true
        // even for a purely cloud-backed one). Probe the on-device-specific API
        // where it exists (API 33+); below that, on-device STT is not exposed.
        val onDevice = onDeviceRecognitionAvailable()
        // Always emit immediately (device default), then refine with the recognizer's
        // REAL supported-locale set on API 33+, mirroring iOS supportedLocales().
        emitAvail(onDevice, listOf(Locale.getDefault().toLanguageTag()))
        querySupportedLocales(
            onResult = { langs -> if (langs.isNotEmpty()) emitAvail(onDevice, langs) },
            onUnknown = {},
        )
    }

    private fun emitAvail(onDevice: Boolean, locales: List<String>) = emit(
        mapOf(
            "event" to "avail",
            "onDeviceAvailable" to onDevice,
            "supportedLocales" to locales,
        ),
    )

    /** True OFFLINE recognition support (API 33+), mirroring iOS supportsOnDeviceRecognition. */
    private fun onDeviceRecognitionAvailable(): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            SpeechRecognizer.isOnDeviceRecognitionAvailable(context)
        } else {
            false
        }

    override fun onCancel(arguments: Any?) {
        sink = null
    }

    private fun emit(map: Map<String, Any?>) = main.post { sink?.success(map) }

    private fun emitError(cid: String, kind: String, message: String) = emit(
        mapOf("event" to "error", "captureId" to cid, "kind" to kind, "message" to message),
    )

    // ── RecognitionListener — delta→completed; partials are display-only ─────

    private fun listener(cid: String, localeTag: String) = object : RecognitionListener {
        override fun onReadyForSpeech(params: Bundle?) {}
        override fun onBeginningOfSpeech() {}
        override fun onRmsChanged(rmsdB: Float) {}
        override fun onBufferReceived(buffer: ByteArray?) {}
        override fun onEndOfSpeech() {}

        override fun onPartialResults(partialResults: Bundle?) {
            val text = firstResult(partialResults) ?: return
            // S5-R19: partials are display-only.
            emit(mapOf("event" to "partial", "captureId" to cid, "text" to text))
        }

        override fun onResults(results: Bundle?) {
            val text = firstResult(results) ?: ""
            // Mean of the per-segment scores, mirroring iOS (segments.map(confidence)
            // .average()) — not just the first score (S5 STT parity).
            val confidence = averageConfidence(
                results?.getFloatArray(SpeechRecognizer.CONFIDENCE_SCORES),
            )
            emit(
                mapOf(
                    "event" to "final",
                    "captureId" to cid,
                    "transcript" to text,
                    "onDevice" to true,
                    "confidence" to confidence,
                    "localeTag" to localeTag,
                    "audioMillis" to 0,
                    "reason" to "completed",
                ),
            )
            teardownRecognizer()
        }

        override fun onError(error: Int) {
            // Diagnostics: the RAW SpeechRecognizer code + the mapped kind. `adb logcat -s QcueStt:*`
            // pins exactly why a take failed (e.g. 7=NO_MATCH, 12=LANGUAGE_UNAVAILABLE, 5=CLIENT).
            Log.w(TAG, "onError: code=$error kind=${mapSttError(error)} locale=$localeTag")
            emitError(cid, mapSttError(error), "recognizer error $error")
            teardownRecognizer()
        }

        override fun onEvent(eventType: Int, params: Bundle?) {}
    }

    private fun firstResult(b: Bundle?): String? =
        b?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull()

    /** Maps Android SpeechRecognizer error codes to the closed SttErrorKind set. */
    private fun mapSttError(code: Int): String = when (code) {
        SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS -> "permission"
        SpeechRecognizer.ERROR_NETWORK,
        SpeechRecognizer.ERROR_NETWORK_TIMEOUT -> "network"
        SpeechRecognizer.ERROR_NO_MATCH,
        SpeechRecognizer.ERROR_SPEECH_TIMEOUT -> "noSpeech"
        SpeechRecognizer.ERROR_LANGUAGE_NOT_SUPPORTED,
        SpeechRecognizer.ERROR_LANGUAGE_UNAVAILABLE -> "unsupportedLocale"
        SpeechRecognizer.ERROR_RECOGNIZER_BUSY -> "unavailable"
        else -> "osError"
    }

    companion object {
        const val RECORD_AUDIO_REQUEST = 0x51C0
        const val TAG = "QcueStt"

        /**
         * v0.2.2 mic fix — whether to FORCE `EXTRA_PREFER_OFFLINE`. Only when the device genuinely has
         * an on-device recognizer ([onDeviceRecognitionAvailable]); otherwise we must NOT force offline,
         * or `startListening` errors immediately on phones without the offline language pack (the
         * "mic won't open / records nothing" bug). Pure so it's unit-testable without a device.
         */
        fun shouldPreferOffline(onDeviceAvailable: Boolean): Boolean = onDeviceAvailable

        /**
         * Mean of the recognizer's per-segment confidence scores, mirroring iOS's
         * averaged `SFTranscriptionSegment.confidence`. Null when absent/empty so
         * the wire field stays nullable (never a fabricated 0.0).
         */
        fun averageConfidence(scores: FloatArray?): Double? {
            if (scores == null || scores.isEmpty()) return null
            var sum = 0.0
            for (s in scores) sum += s
            return sum / scores.size
        }

        /**
         * Does [supported] (a recognizer language tag) satisfy a request for the
         * [wanted] locale? Tolerant of `_`/`-` and case, and matches by language
         * subtag (a device that supports "en-US" satisfies a request for "en").
         */
        fun localeMatches(supported: String, wanted: String): Boolean {
            fun norm(s: String) = s.replace('_', '-').lowercase()
            val a = norm(supported)
            val b = norm(wanted)
            if (a == b) return true
            return a.substringBefore('-') == b.substringBefore('-')
        }

        /** Triggers the RECORD_AUDIO runtime prompt; the Dart status read follows. */
        fun requestRecordAudio(activity: Activity) {
            ActivityCompat.requestPermissions(
                activity, arrayOf(Manifest.permission.RECORD_AUDIO), RECORD_AUDIO_REQUEST,
            )
        }
    }
}
