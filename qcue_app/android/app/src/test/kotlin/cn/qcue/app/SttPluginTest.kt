package cn.qcue.app

import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.kotlin.any
import org.mockito.kotlin.mock
import org.mockito.kotlin.verify
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment

/**
 * QCue S5-R3/R4 — the Android STT bridge guards the schema version and maps errors
 * to the closed kind set. (native-unverified-here: requires the Android toolchain;
 * not run on the Linux CI host.)
 */
@RunWith(RobolectricTestRunner::class)
class SttPluginTest {

    private fun plugin(): SttPlugin {
        val ctx = RuntimeEnvironment.getApplication()
        return SttPlugin(ctx, mock(), mock())
    }

    @Test
    fun `unknown schema version is rejected with versionMismatch`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(
            MethodCall("isAvailable", mapOf("schemaVersion" to 999)),
            result,
        )
        verify(result).error(eqStr("versionMismatch"), any(), any())
    }

    @Test
    fun `isAvailable returns a boolean for the current schema version`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(
            MethodCall("isAvailable", mapOf("schemaVersion" to QcueChannels.SCHEMA_VERSION)),
            result,
        )
        verify(result).success(any())
    }

    @Test
    fun `requestPermission reports denied without the RECORD_AUDIO grant`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(
            MethodCall(
                "requestPermission",
                mapOf("schemaVersion" to QcueChannels.SCHEMA_VERSION),
            ),
            result,
        )
        // Robolectric grants no runtime permission by default → "denied".
        verify(result).success(eqStr("denied"))
    }

    @Test
    fun `schema version constant matches the channel contract`() {
        assertEquals(1, QcueChannels.SCHEMA_VERSION)
        assertNotNull(QcueChannels.STT_EVENTS)
    }

    // ── v0.2.1 STT parity: averaged confidence (mirrors iOS) ──────────────────

    @Test
    fun `averageConfidence is null for null or empty scores`() {
        assertEquals(null, SttPlugin.averageConfidence(null))
        assertEquals(null, SttPlugin.averageConfidence(floatArrayOf()))
    }

    @Test
    fun `averageConfidence is the mean of the segment scores`() {
        assertEquals(0.5, SttPlugin.averageConfidence(floatArrayOf(0.4f, 0.6f))!!, 1e-6)
        assertEquals(0.9, SttPlugin.averageConfidence(floatArrayOf(0.9f))!!, 1e-6)
        assertEquals(
            0.5,
            SttPlugin.averageConfidence(floatArrayOf(0.2f, 0.4f, 0.6f, 0.8f))!!,
            1e-6,
        )
    }

    // ── v0.2.1 STT parity: locale-aware availability ──────────────────────────

    @Test
    fun `localeMatches tolerates separators, case, and language subtag`() {
        assertTrue(SttPlugin.localeMatches("en-US", "en-US"))
        assertTrue(SttPlugin.localeMatches("en_US", "en-US")) // underscore form
        assertTrue(SttPlugin.localeMatches("EN-us", "en-US")) // case
        assertTrue(SttPlugin.localeMatches("en-GB", "en")) // language subtag
        assertTrue(SttPlugin.localeMatches("zh-CN", "zh-Hans-CN".substringBefore('-')))
        assertFalse(SttPlugin.localeMatches("fr-FR", "en-US"))
    }

    @Test
    fun `isAvailable with a localeTag still completes the result (never hangs)`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(
            MethodCall(
                "isAvailable",
                mapOf("schemaVersion" to QcueChannels.SCHEMA_VERSION, "localeTag" to "en-US"),
            ),
            result,
        )
        verify(result).success(any())
    }

    // ── v0.2.2 mic fix: adaptive offline preference ───────────────────────────

    @Test
    fun `prefer-offline is forced ONLY when on-device recognition is available`() {
        // The bug: forcing EXTRA_PREFER_OFFLINE on a device with no offline pack made startListening
        // error immediately. The fix gates it on genuine on-device availability.
        assertTrue(SttPlugin.shouldPreferOffline(true))
        assertFalse(SttPlugin.shouldPreferOffline(false))
    }

    private fun eqStr(s: String): String = org.mockito.kotlin.eq(s)
}
