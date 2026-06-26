import Flutter
import Foundation
import UserNotifications

/// QCue S5-R33/R34 — the thin iOS local-notification bridge.
///
/// Shows a notification for the three closed kinds over `qcue/notif`; a tap is
/// delivered to `userNotificationCenter(_:didReceive:)` which forwards the
/// {kind, route} over `qcue/notif/events` for go_router (S5-R34). The route map
/// is opaque to native — Dart owns navigation. Push/APNs is roadmap;
/// `registerPushToken` is a documented no-op stub.
///
/// NO business logic lives here (S5-R1). NOTE (native-unverified-here): code-complete
/// for the iOS toolchain; NOT compiled/run on the Linux CI host. An XCTest accompanies it.
final class NotifHandler: NSObject, FlutterStreamHandler, UNUserNotificationCenterDelegate {

    private var sink: FlutterEventSink?

    static func register(messenger: FlutterBinaryMessenger) -> NotifHandler {
        let handler = NotifHandler()
        let method = FlutterMethodChannel(name: QcueChannels.notif, binaryMessenger: messenger)
        let events = FlutterEventChannel(name: QcueChannels.notifEvents, binaryMessenger: messenger)
        method.setMethodCallHandler { [weak handler] call, result in
            handler?.handle(call, result: result)
        }
        events.setStreamHandler(handler)
        UNUserNotificationCenter.current().delegate = handler
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
        let args = call.arguments as? [String: Any] ?? [:]
        switch call.method {
        case "requestPermission":
            requestPermission { result($0) }
        case "show":
            show(args)
            result(nil)
        case "cancelKind":
            if let kind = args["kind"] as? String {
                UNUserNotificationCenter.current()
                    .removeDeliveredNotifications(withIdentifiers: [kind])
            }
            result(nil)
        case "registerPushToken":
            result(nil) // roadmap stub (S5-R35)
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    private func requestPermission(_ done: @escaping (String) -> Void) {
        UNUserNotificationCenter.current()
            .requestAuthorization(options: [.alert, .sound, .badge]) { granted, _ in
                DispatchQueue.main.async { done(granted ? "granted" : "denied") }
            }
    }

    private func show(_ args: [String: Any]) {
        guard let kind = args["kind"] as? String else { return }
        let content = UNMutableNotificationContent()
        content.title = args["title"] as? String ?? "QCue"
        content.body = args["body"] as? String ?? ""
        // S5-R34 — carry the route so the tap deep-links.
        content.userInfo = ["kind": kind, "route": args["route"] ?? [:]]
        // Stable identifier per kind → tapping the same notif twice navigates once.
        let req = UNNotificationRequest(identifier: kind, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(req)
    }

    // MARK: - Tap deep-link (S5-R34)

    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let info = response.notification.request.content.userInfo
        if let kind = info["kind"] as? String {
            let route = info["route"] as? [String: String] ?? [:]
            sink?(["kind": kind, "route": route])
        }
        completionHandler()
    }

    // Show banners while foregrounded.
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        // `.banner` is iOS 14+; `.alert` is the iOS 13 equivalent (Runner floor is 13.0).
        if #available(iOS 14.0, *) {
            completionHandler([.banner, .sound])
        } else {
            completionHandler([.alert, .sound])
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
