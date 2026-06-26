import Flutter
import Foundation
#if canImport(WidgetKit)
import WidgetKit
#endif

/// QCue S5-R46/R47 — the thin iOS home-screen-widget bridge over `qcue/widget`.
///
/// `setCount` writes the NON-SENSITIVE today-count into the App Group container
/// the WidgetKit widget reads (S5-R46 — never idea/wiki bodies on the home
/// screen); `reloadTimelines` asks WidgetKit to refresh (S5-R47). Widget-tap
/// intents (compose via `widgetURL`, quickCapture via an App Intent) re-enter the
/// app and are forwarded over `qcue/widget/events` from the SceneDelegate.
///
/// NO business logic lives here (S5-R1). NOTE (native-unverified-here): code-complete
/// for the iOS toolchain; NOT compiled/run on the Linux CI host. An XCTest accompanies it.
final class WidgetHandler: NSObject, FlutterStreamHandler {

    private var sink: FlutterEventSink?

    static func register(messenger: FlutterBinaryMessenger) -> WidgetHandler {
        let handler = WidgetHandler()
        let method = FlutterMethodChannel(name: QcueChannels.widget, binaryMessenger: messenger)
        let events = FlutterEventChannel(name: QcueChannels.widgetEvents, binaryMessenger: messenger)
        method.setMethodCallHandler { [weak handler] call, result in
            handler?.handle(call, result: result)
        }
        events.setStreamHandler(handler)
        return handler
    }

    #if DEBUG
    func testHandle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        handle(call, result: result)
    }
    #endif

    /// Forward a widget tap (compose / quickCapture) to Dart — called from the
    /// SceneDelegate when the app opens via a `qcue://` widget URL (S5-R45).
    func deliverTap(action: String, args: [String: Any] = [:]) {
        sink?(["action": action, "args": args])
    }

    private func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        guard QcueChannels.versionOk(call.arguments) else {
            result(QcueChannels.versionError()); return
        }
        let args = call.arguments as? [String: Any] ?? [:]
        switch call.method {
        case "setCount":
            SharedContainer.setWidgetCount(args["count"] as? Int ?? 0)
            result(nil)
        case "reloadTimelines":
            if #available(iOS 14.0, *) {
                #if canImport(WidgetKit)
                WidgetCenter.shared.reloadAllTimelines()
                #endif
            }
            result(nil)
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    func onListen(withArguments _: Any?, eventSink events: @escaping FlutterEventSink) -> FlutterError? {
        sink = events
        return nil
    }

    func onCancel(withArguments _: Any?) -> FlutterError? {
        sink = nil
        return nil
    }
}
