import Flutter
import Foundation

/// QCue S5-R3 — channel-name + schema-version constants shared by the iOS native
/// handlers. These MUST match the Dart `QcueChannels` (lib/core/native/channels.dart)
/// and the Android `QcueChannels` exactly.
enum QcueChannels {
    static let schemaVersion = 1

    static let stt = "qcue/stt"
    static let sttEvents = "qcue/stt/events"
    static let secure = "qcue/secure"

    static let share = "qcue/share"
    static let shareEvents = "qcue/share/events"
    static let widget = "qcue/widget"
    static let widgetEvents = "qcue/widget/events"
    static let notif = "qcue/notif"
    static let notifEvents = "qcue/notif/events"
    static let background = "qcue/background"

    /// The shared App Group container id (Share Extension + WidgetKit + the app).
    /// MUST match the App Group entitlement on every target.
    static let appGroup = "group.cn.qcue.shared"

    /// S5-R3 — verify the inbound payload carries the supported major version.
    static func versionOk(_ args: Any?) -> Bool {
        guard let map = args as? [String: Any] else { return false }
        return (map["schemaVersion"] as? Int) == schemaVersion
    }

    static func versionError() -> FlutterError {
        FlutterError(code: "versionMismatch", message: "unsupported schemaVersion",
                     details: ["kind": "versionMismatch", "retryable": false])
    }
}
