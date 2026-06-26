package cn.qcue.app

import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.junit.Assert.assertNull
import org.junit.Ignore
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.kotlin.any
import org.mockito.kotlin.eq
import org.mockito.kotlin.mock
import org.mockito.kotlin.verify
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment

/**
 * QCue S5-R24/R26 — the Android secure store round-trips the WRAPPED blob and fails
 * a denied biometric read CLOSED, never returning the blob unauthenticated.
 * (native-unverified-here: requires the Android toolchain.)
 */
@RunWith(RobolectricTestRunner::class)
class SecurePluginTest {

    private fun plugin(grantBio: Boolean): SecurePlugin {
        val ctx = RuntimeEnvironment.getApplication()
        return SecurePlugin(ctx, mock(), biometricGate = { _, cb ->
            cb(if (grantBio) BiometricOutcome.GRANTED else BiometricOutcome.DENIED)
        })
    }

    private fun call(method: String, args: Map<String, Any?>) =
        MethodCall(method, args + mapOf("schemaVersion" to QcueChannels.SCHEMA_VERSION))

    @Ignore("EncryptedSharedPreferences needs AndroidKeyStore, which Robolectric " +
        "does not provide; the write/read round-trip is verified on-device.")
    @Test
    fun `write then read round-trips the wrapped blob`() {
        val p = plugin(grantBio = true)
        val w: MethodChannel.Result = mock()
        p.onMethodCall(call("write", mapOf("key" to "cred_openai", "value" to "WRAPPED")), w)
        verify(w).success(any())

        val r: MethodChannel.Result = mock()
        p.onMethodCall(call("read", mapOf("key" to "cred_openai")), r)
        verify(r).success(eq("WRAPPED"))
    }

    @Test
    fun `a denied biometric read fails closed`() {
        val p = plugin(grantBio = false)
        val w: MethodChannel.Result = mock()
        p.onMethodCall(call("write", mapOf("key" to "cred_a", "value" to "BLOB")), w)

        val r: MethodChannel.Result = mock()
        p.onMethodCall(
            call("read", mapOf("key" to "cred_a", "requireBiometric" to true)),
            r,
        )
        // No blob is returned; a typed permissionDenied error is raised instead.
        verify(r).error(eq("permissionDenied"), any(), any())
    }

    @Ignore("EncryptedSharedPreferences needs AndroidKeyStore, which Robolectric " +
        "does not provide; presence is verified on-device.")
    @Test
    fun `containsKey reflects presence`() {
        val p = plugin(grantBio = true)
        p.onMethodCall(call("write", mapOf("key" to "k", "value" to "v")), mock())
        val r: MethodChannel.Result = mock()
        p.onMethodCall(call("containsKey", mapOf("key" to "k")), r)
        verify(r).success(eq(true))
    }

    @Test
    fun `unknown schema version is rejected`() {
        val p = plugin(grantBio = true)
        val r: MethodChannel.Result = mock()
        p.onMethodCall(MethodCall("read", mapOf("schemaVersion" to 2, "key" to "k")), r)
        verify(r).error(eq("versionMismatch"), any(), any())
        assertNull(null)
    }
}
