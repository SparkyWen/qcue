package cn.qcue.app

import android.content.Context
import android.content.SharedPreferences
import android.os.Build
import androidx.biometric.BiometricManager
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/**
 * The outcome of a biometric gate. Mirrors iOS's LAError handling, which maps a
 * user-cancel (userCancel/appCancel/systemCancel) to the `cancelled` error kind and
 * any other auth failure to `permissionDenied`. Both [CANCELLED] and [DENIED] fail
 * CLOSED (no blob returned); only the surfaced error kind differs (S5-R26).
 */
enum class BiometricOutcome { GRANTED, CANCELLED, DENIED }

/**
 * QCue S5-R24/R25/R27 (D9) — the thin Android secure key store bridge over
 * `qcue/secure`.
 *
 * Stores the WRAPPED (AES-GCM ciphertext) BYOK blob in AndroidX
 * [EncryptedSharedPreferences], whose master key lives in the AndroidKeyStore
 * (StrongBox-backed where present). The plaintext key NEVER lives here — only the
 * already-wrapped blob crosses this channel (S5-R24); decryption happens in the Rust
 * `secrets` crate into a zeroize-on-drop buffer. Values are NEVER logged (S5-R28).
 *
 * The biometric gate (S5-R26) is enforced on read: a `requireBiometric=true` read
 * prompts via BiometricPrompt; a denial fails CLOSED (a PermissionDenied error → the
 * Dart facade returns null). Wiring the BiometricPrompt requires a FragmentActivity,
 * supplied by the host; here the gate is represented by [biometricGate].
 *
 * NOTE (native-unverified-here): code-complete; NOT compiled/unit-run on the Linux CI
 * host (no Android SDK). Robolectric tests accompany it.
 */
class SecurePlugin(
    private val context: Context,
    private val channel: MethodChannel,
    /**
     * Resolves the biometric prompt with a [BiometricOutcome]. Fails CLOSED by default (DENIED): a
     * credential store must never auto-grant if a future caller forgets to wire the real gate. All
     * current call sites (MainActivity, BackgroundPlugin, the test) pass an explicit gate, so live
     * behavior is unchanged.
     */
    private val biometricGate: (reason: String, onResult: (BiometricOutcome) -> Unit) -> Unit =
        { _, cb -> cb(BiometricOutcome.DENIED) },
) : MethodChannel.MethodCallHandler {

    fun attach() = channel.setMethodCallHandler(this)
    fun detach() = channel.setMethodCallHandler(null)

    private val prefs: SharedPreferences by lazy {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .apply {
                // StrongBox where available (S5-R25); the wrapped blob is
                // ThisDeviceOnly (no auto-backup — see android:allowBackup=false).
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) setRequestStrongBoxBacked(true)
            }
            .build()
        EncryptedSharedPreferences.create(
            context,
            "qcue_secure_keys",
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )
    }

    /** Whether a strong biometric or device credential is enrolled (gate UI hint). */
    private fun biometricAvailable(): Boolean = try {
        BiometricManager.from(context).canAuthenticate(
            BiometricManager.Authenticators.BIOMETRIC_STRONG or
                BiometricManager.Authenticators.DEVICE_CREDENTIAL,
        ) == BiometricManager.BIOMETRIC_SUCCESS
    } catch (e: Exception) {
        false
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        val version = call.argument<Int>("schemaVersion") ?: 0
        if (version != QcueChannels.SCHEMA_VERSION) {
            result.error(
                "versionMismatch", "unsupported schemaVersion $version",
                mapOf("kind" to "versionMismatch", "retryable" to false),
            )
            return
        }
        try {
            when (call.method) {
                "write" -> {
                    val key = call.argument<String>("key")!!
                    val value = call.argument<String>("value")!!
                    // Write does NOT prompt (S5-R26) so the key can be cached on sync.
                    prefs.edit().putString(key, value).apply()
                    result.success(null)
                }
                "read" -> {
                    val key = call.argument<String>("key")!!
                    val requireBio = call.argument<Boolean>("requireBiometric") ?: false
                    val reason = call.argument<String>("reason") ?: "Unlock your key"
                    if (requireBio) {
                        biometricGate(reason) { outcome ->
                            when (outcome) {
                                BiometricOutcome.GRANTED ->
                                    result.success(prefs.getString(key, null))
                                // Both branches fail closed (S5-R26): no blob is
                                // returned unauthenticated. The kind matches iOS —
                                // user-cancel → cancelled, otherwise permissionDenied.
                                BiometricOutcome.CANCELLED -> result.error(
                                    "cancelled", "biometric cancelled",
                                    mapOf("kind" to "cancelled", "retryable" to false),
                                )
                                BiometricOutcome.DENIED -> result.error(
                                    "permissionDenied", "biometric denied",
                                    mapOf("kind" to "permissionDenied", "retryable" to false),
                                )
                            }
                        }
                    } else {
                        result.success(prefs.getString(key, null))
                    }
                }
                "delete" -> {
                    val key = call.argument<String>("key")!!
                    prefs.edit().remove(key).apply()
                    result.success(null)
                }
                "containsKey" -> {
                    val key = call.argument<String>("key")!!
                    result.success(prefs.contains(key))
                }
                "biometricAvailable" -> result.success(biometricAvailable())
                else -> result.notImplemented()
            }
        } catch (e: Exception) {
            // S5-R28: the message must never echo a value; only a generic note.
            result.error("osError", "secure store error", mapOf("kind" to "osError", "retryable" to false))
        }
    }
}
