package cn.qcue.app

/**
 * QCue S5-R3 — the channel-name + schema-version constants shared by the Android
 * native handlers. These MUST match the Dart `QcueChannels` (lib/core/native/
 * channels.dart) and the iOS `QcueChannels` exactly.
 */
object QcueChannels {
    const val SCHEMA_VERSION = 1

    const val STT = "qcue/stt"
    const val STT_EVENTS = "qcue/stt/events"
    const val SECURE = "qcue/secure"

    const val SHARE = "qcue/share"
    const val SHARE_EVENTS = "qcue/share/events"
    const val WIDGET = "qcue/widget"
    const val WIDGET_EVENTS = "qcue/widget/events"
    const val NOTIF = "qcue/notif"
    const val NOTIF_EVENTS = "qcue/notif/events"
    const val BACKGROUND = "qcue/background"
    const val INSTALLER = "qcue/installer"
}
