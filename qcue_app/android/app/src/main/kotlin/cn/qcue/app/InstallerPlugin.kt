package cn.qcue.app

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.core.content.FileProvider
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.io.File

/**
 * QCue AU-R21 — fire the OS package-installer for a downloaded APK (`qcue/installer`). Android REQUIRES
 * the user to tap "Install" (and grant install-unknown-apps once); there is no silent install for a
 * normal sideloaded app, by design. The APK is served from the app's own cache dir (where the Dart
 * side wrote it) through a FileProvider content:// URI — a `file://` URI is rejected on API 24+.
 *
 * No business logic lives here (S5-R1). NOTE (native-unverified-here): code-complete for the Android
 * toolchain; NOT compiled/run on the Linux CI host (no Android SDK) — verified at the Android build.
 */
class InstallerPlugin(
    private val context: Context,
    private val method: MethodChannel,
) : MethodChannel.MethodCallHandler {

    fun attach() = method.setMethodCallHandler(this)
    fun detach() = method.setMethodCallHandler(null)

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        if ((call.argument<Int>("schemaVersion") ?: 0) != QcueChannels.SCHEMA_VERSION) {
            result.error("versionMismatch", "unsupported schemaVersion",
                mapOf("kind" to "versionMismatch", "retryable" to false))
            return
        }
        when (call.method) {
            "installApk" -> {
                val path = call.argument<String>("filePath")
                if (path == null) {
                    result.error("osError", "filePath missing",
                        mapOf("kind" to "osError", "retryable" to false))
                    return
                }
                try {
                    val file = File(path)
                    val uri: Uri = FileProvider.getUriForFile(
                        context, "${context.packageName}.fileprovider", file)
                    val intent = Intent(Intent.ACTION_VIEW).apply {
                        setDataAndType(uri, "application/vnd.android.package-archive")
                        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    }
                    context.startActivity(intent)
                    result.success(null)
                } catch (e: Exception) {
                    result.error("osError", e.message,
                        mapOf("kind" to "osError", "retryable" to false))
                }
            }
            else -> result.notImplemented()
        }
    }
}
