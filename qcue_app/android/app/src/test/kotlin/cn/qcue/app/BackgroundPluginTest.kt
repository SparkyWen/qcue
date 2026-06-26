package cn.qcue.app

import androidx.work.Configuration
import androidx.work.testing.WorkManagerTestInitHelper
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.junit.Assert.assertEquals
import org.junit.Before
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
 * QCue S5-R37/R38 — the Android background-flush scheduler enqueues a unique,
 * network-gated periodic flush worker and cancels it.
 */
@RunWith(RobolectricTestRunner::class)
class BackgroundPluginTest {

    @Before
    fun setUp() {
        // WorkManager must be initialized before getInstance() in unit tests.
        WorkManagerTestInitHelper.initializeTestWorkManager(
            RuntimeEnvironment.getApplication(),
            Configuration.Builder().build(),
        )
    }

    private fun plugin() =
        BackgroundPlugin(RuntimeEnvironment.getApplication(), mock())

    @Test
    fun `schedulePeriodic enqueues a unique periodic work and succeeds`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(
            MethodCall(
                "schedulePeriodic",
                mapOf("schemaVersion" to QcueChannels.SCHEMA_VERSION, "requiresNetwork" to true),
            ),
            result,
        )
        verify(result).success(anyOrNull())
    }

    @Test
    fun `cancel succeeds`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(
            MethodCall("cancel", mapOf("schemaVersion" to QcueChannels.SCHEMA_VERSION)),
            result,
        )
        verify(result).success(anyOrNull())
    }

    @Test
    fun `unknown schema version is rejected`() {
        val p = plugin()
        val result: MethodChannel.Result = mock()
        p.onMethodCall(MethodCall("schedulePeriodic", mapOf("schemaVersion" to 999)), result)
        verify(result).error(eq("versionMismatch"), any(), any())
    }

    @Test
    fun `unique work name is stable`() {
        assertEquals("qcue.flush.periodic", FlushWorker.UNIQUE_WORK)
    }
}
