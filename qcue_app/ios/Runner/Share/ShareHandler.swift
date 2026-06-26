import Flutter
import Foundation

/// QCue S5-R42/R43 — the thin iOS share-sheet bridge.
///
/// The Share Extension (a separate process) writes the shared item to the App
/// Group container; this handler DRAINS it on launch/resume over `qcue/share`
/// (`drainPending`) and also re-emits it over `qcue/share/events` for the live
/// listener. The Dart side enqueues a clip capture (origin='share'|'web'),
/// offline-safe. NO business logic lives here (S5-R1) — the blob is staged/
/// forwarded verbatim; capture/dedup/S2-fencing happen downstream.
///
/// NOTE (native-unverified-here): code-complete for the iOS toolchain; NOT compiled
/// or run on the Linux CI host. An XCTest accompanies it.
final class ShareHandler: NSObject, FlutterStreamHandler {

    private var sink: FlutterEventSink?

    static func register(messenger: FlutterBinaryMessenger) -> ShareHandler {
        let handler = ShareHandler()
        let method = FlutterMethodChannel(name: QcueChannels.share, binaryMessenger: messenger)
        let events = FlutterEventChannel(name: QcueChannels.shareEvents, binaryMessenger: messenger)
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

    private func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        guard QcueChannels.versionOk(call.arguments) else {
            result(QcueChannels.versionError()); return
        }
        switch call.method {
        case "drainPending":
            result(SharedContainer.drainSharedItems())
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    /// Re-emit any staged items live (called from the SceneDelegate on resume).
    func emitPending() {
        for item in SharedContainer.drainSharedItems() {
            sink?(item)
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
