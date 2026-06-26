package cn.qcue.app

import android.content.Intent
import android.net.Uri
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import io.flutter.embedding.android.FlutterFragmentActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodChannel

/**
 * QCue S5 — registers the native capability plugins on the engine's method/event
 * channels and routes inbound intents (share, widget/notification deep links).
 * The handlers are thin OS-API bridges (S5-R1); all policy + persistence lives in
 * the shared Rust core reached over flutter_rust_bridge / the offline queue.
 *
 * NOTE (native-unverified-here): code-complete for the Android toolchain; NOT
 * compiled/run on the Linux CI host (no Android SDK).
 */
class MainActivity : FlutterFragmentActivity() {
    private var stt: SttPlugin? = null
    private var secure: SecurePlugin? = null
    private var share: SharePlugin? = null
    private var widget: WidgetPlugin? = null
    private var notif: NotifPlugin? = null
    private var background: BackgroundPlugin? = null
    private var installer: InstallerPlugin? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        val messenger = flutterEngine.dartExecutor.binaryMessenger

        stt = SttPlugin(
            context = applicationContext,
            method = MethodChannel(messenger, QcueChannels.STT),
            events = EventChannel(messenger, QcueChannels.STT_EVENTS),
            activity = this,
        ).also { it.attach() }

        secure = SecurePlugin(
            context = applicationContext,
            channel = MethodChannel(messenger, QcueChannels.SECURE),
            biometricGate = ::showBiometricPrompt,
        ).also { it.attach() }

        share = SharePlugin(
            context = applicationContext,
            method = MethodChannel(messenger, QcueChannels.SHARE),
            events = EventChannel(messenger, QcueChannels.SHARE_EVENTS),
        ).also { it.attach() }

        widget = WidgetPlugin(
            context = applicationContext,
            method = MethodChannel(messenger, QcueChannels.WIDGET),
            events = EventChannel(messenger, QcueChannels.WIDGET_EVENTS),
        ).also { it.attach() }

        notif = NotifPlugin(
            context = applicationContext,
            method = MethodChannel(messenger, QcueChannels.NOTIF),
            events = EventChannel(messenger, QcueChannels.NOTIF_EVENTS),
            activity = this,
        ).also { it.attach() }

        background = BackgroundPlugin(
            context = applicationContext,
            method = MethodChannel(messenger, QcueChannels.BACKGROUND),
        ).also { it.attach() }

        // AU-R21 — the APK install-intent bridge (full-update path on Android).
        installer = InstallerPlugin(
            context = applicationContext,
            method = MethodChannel(messenger, QcueChannels.INSTALLER),
        ).also { it.attach() }

        // The launch intent may carry a share / deep-link payload.
        routeIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        routeIntent(intent)
    }

    /**
     * Route a runtime-permission result to whichever plugin requested it
     * (RECORD_AUDIO → STT, POST_NOTIFICATIONS → notifications). Without this the
     * plugins' async `requestPermission` results would never complete.
     */
    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        stt?.onRequestPermissionsResult(requestCode, grantResults)
        notif?.onRequestPermissionsResult(requestCode, grantResults)
    }

    /** Dispatch an inbound intent to the right channel (share / widget / notif). */
    private fun routeIntent(intent: Intent?) {
        if (intent == null) return
        when {
            intent.action == Intent.ACTION_SEND -> share?.handleSendIntent(intent)
            intent.action == NotifPlugin.ACTION_TAP -> {
                val kind = intent.getStringExtra(NotifPlugin.EXTRA_KIND) ?: return
                // MainActivity is exported (LAUNCHER + qcue:// VIEW), so any app can deliver this internal
                // tap action. The Dart side allowlists `kind`; here we additionally sanitize each route
                // value to a single safe token (no '/', bounded length) so a hostile extra can't be
                // interpolated into a go_router path (e.g. `../../login`) to drive arbitrary navigation.
                val safe = Regex("^[A-Za-z0-9_-]{1,64}$")
                val route = intent.extras?.keySet().orEmpty()
                    .filter { it.startsWith("route_") }
                    .associate { it.removePrefix("route_") to (intent.getStringExtra(it) ?: "") }
                    .filterValues { it.matches(safe) }
                notif?.deliverTap(kind, route)
            }
            intent.action == Intent.ACTION_VIEW -> routeDeepLink(intent.data)
        }
    }

    private fun routeDeepLink(uri: Uri?) {
        if (uri == null) return
        // Accept BOTH the legacy custom scheme (qcue://, kept during the App Links transition) AND verified
        // https App Links under app.qcue.cn/applink (bound to this signed app via assetlinks.json — see
        // android/APP_LINKS.md). The OAuth redirect (/applink/oauth2redirect) is intentionally NOT handled
        // here: it is path-scoped to flutter_appauth's RedirectUriReceiverActivity in AndroidManifest.xml,
        // so this allowlist stays narrow and non-overlapping (no ambiguous routing mid-sign-in).
        val isAppLink = uri.scheme == "https" && uri.host == "app.qcue.cn" &&
            uri.path?.startsWith("/applink/") == true
        if (uri.scheme != "qcue" && !isAppLink) return
        // qcue:// — the host carries the target (capture/widget); the path is the action.
        when (uri.host) {
            "capture" -> if (uri.path == "/compose") widget?.deliverTap("compose")
            "widget" -> if (uri.path == "/quickCapture") widget?.deliverTap("quickCapture")
        }
        // https App Link — the whole target lives in the path under /applink (host is app.qcue.cn).
        // Mirrors the qcue:// allowlist above so a hostile link can't drive arbitrary taps.
        if (isAppLink) when (uri.path?.removePrefix("/applink")) {
            "/capture/compose" -> widget?.deliverTap("compose")
            "/widget/quickCapture" -> widget?.deliverTap("quickCapture")
        }
    }

    /**
     * S5-R26 (D9) — the real biometric gate for BYOK vault reads. Shows a
     * BiometricPrompt (strong biometric OR device credential) and resolves the
     * SecurePlugin read with the outcome. Fails CLOSED: if nothing is enrolled, or
     * the prompt errors/cancels, [onResult] is false and the wrapped blob is never
     * returned unauthenticated. Replaces the previous always-true default gate — a
     * fail-OPEN security regression. Requires the FragmentActivity host.
     */
    private fun showBiometricPrompt(reason: String, onResult: (BiometricOutcome) -> Unit) {
        runOnUiThread {
            val authenticators = BiometricManager.Authenticators.BIOMETRIC_STRONG or
                BiometricManager.Authenticators.DEVICE_CREDENTIAL
            if (BiometricManager.from(this).canAuthenticate(authenticators) !=
                BiometricManager.BIOMETRIC_SUCCESS
            ) {
                onResult(BiometricOutcome.DENIED) // nothing enrolled → fail closed (S5-R26)
                return@runOnUiThread
            }
            var settled = false
            fun settle(outcome: BiometricOutcome) {
                if (!settled) {
                    settled = true
                    onResult(outcome)
                }
            }
            val prompt = BiometricPrompt(
                this,
                ContextCompat.getMainExecutor(this),
                object : BiometricPrompt.AuthenticationCallback() {
                    override fun onAuthenticationSucceeded(
                        result: BiometricPrompt.AuthenticationResult,
                    ) = settle(BiometricOutcome.GRANTED)

                    // Distinguish a user-driven cancel from a real auth failure so
                    // the surfaced error kind matches iOS (cancelled vs
                    // permissionDenied). Either way the read fails closed.
                    override fun onAuthenticationError(
                        errorCode: Int,
                        errString: CharSequence,
                    ) = settle(
                        if (errorCode == BiometricPrompt.ERROR_USER_CANCELED ||
                            errorCode == BiometricPrompt.ERROR_NEGATIVE_BUTTON ||
                            errorCode == BiometricPrompt.ERROR_CANCELED
                        ) {
                            BiometricOutcome.CANCELLED
                        } else {
                            BiometricOutcome.DENIED
                        },
                    )
                },
            )
            val info = BiometricPrompt.PromptInfo.Builder()
                .setTitle("Unlock your key")
                .setSubtitle(reason)
                .setAllowedAuthenticators(authenticators)
                .build()
            try {
                prompt.authenticate(info)
            } catch (e: Exception) {
                settle(BiometricOutcome.DENIED)
            }
        }
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        stt?.detach(); secure?.detach(); share?.detach()
        widget?.detach(); notif?.detach(); background?.detach(); installer?.detach()
        stt = null; secure = null; share = null
        widget = null; notif = null; background = null; installer = null
        super.cleanUpFlutterEngine(flutterEngine)
    }
}
