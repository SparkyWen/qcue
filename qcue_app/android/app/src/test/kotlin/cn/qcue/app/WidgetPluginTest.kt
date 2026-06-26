package cn.qcue.app

import android.content.Context
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
 * QCue S5-R46/R47 — the Android widget bridge persists only a non-sensitive count
 * and reloads the timeline; tap intents forward compose/quickCapture.
 * (native-unverified-here.)
 */
@RunWith(RobolectricTestRunner::class)
class WidgetPluginTest {

    private fun plugin() =
        WidgetPlugin(RuntimeEnvironment.getApplication(), mock(), mock())

    @Test
    fun `setCount persists only the count (S5-R46 — no body content)`() {
        val ctx = RuntimeEnvironment.getApplication()
        val p = WidgetPlugin(ctx, mock(), mock())
        val result: MethodChannel.Result = mock()
        p.onMethodCall(
            MethodCall(
                "setCount",
                mapOf("schemaVersion" to QcueChannels.SCHEMA_VERSION, "count" to 5),
            ),
            result,
        )
        verify(result).success(anyOrNull())
        val stored = ctx.getSharedPreferences(WidgetPlugin.STORE, Context.MODE_PRIVATE)
            .getInt(WidgetPlugin.KEY_COUNT, -1)
        assertEquals(5, stored)
    }

    @Test
    fun `reloadTimelines succeeds even with no widget instances`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(
            MethodCall("reloadTimelines", mapOf("schemaVersion" to QcueChannels.SCHEMA_VERSION)),
            result,
        )
        verify(result).success(anyOrNull())
    }

    @Test
    fun `unknown schema version is rejected`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(MethodCall("setCount", mapOf("schemaVersion" to 999)), result)
        verify(result).error(eq("versionMismatch"), any(), any())
    }
}
