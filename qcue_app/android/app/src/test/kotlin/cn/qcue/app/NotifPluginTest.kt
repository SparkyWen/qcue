package cn.qcue.app

import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.kotlin.any
import org.mockito.kotlin.anyOrNull
import org.mockito.kotlin.eq
import org.mockito.kotlin.mock
import org.mockito.kotlin.verify
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment

/**
 * QCue S5-R33/R34 — the Android notif bridge shows the three closed kinds, cancels
 * by kind, and stubs push registration. (native-unverified-here.)
 */
@RunWith(RobolectricTestRunner::class)
class NotifPluginTest {

    private fun plugin(): NotifPlugin {
        val p = NotifPlugin(RuntimeEnvironment.getApplication(), mock(), mock())
        p.attach()
        return p
    }

    @Test
    fun `show a dreamComplete notification succeeds`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(
            MethodCall(
                "show",
                mapOf(
                    "schemaVersion" to QcueChannels.SCHEMA_VERSION,
                    "kind" to "dreamComplete",
                    "title" to "QCue improved 3 pages",
                    "body" to "tap to see",
                    "route" to mapOf("id" to "job-1"),
                ),
            ),
            result,
        )
        verify(result).success(anyOrNull())
    }

    @Test
    fun `cancelKind succeeds`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(
            MethodCall(
                "cancelKind",
                mapOf("schemaVersion" to QcueChannels.SCHEMA_VERSION, "kind" to "syncConflict"),
            ),
            result,
        )
        verify(result).success(anyOrNull())
    }

    @Test
    fun `registerPushToken is a no-op stub (roadmap)`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(
            MethodCall("registerPushToken", mapOf("schemaVersion" to QcueChannels.SCHEMA_VERSION)),
            result,
        )
        verify(result).success(anyOrNull())
    }

    @Test
    fun `unknown schema version is rejected`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(MethodCall("show", mapOf("schemaVersion" to 999)), result)
        verify(result).error(eq("versionMismatch"), any(), any())
    }

    @Test
    fun `channel constant matches the contract`() {
        assertEquals(1, QcueChannels.SCHEMA_VERSION)
    }
}
