import BackgroundTasks
import Flutter
import Foundation

/// QCue S5-R37/R38 — the thin iOS background-flush scheduler over `qcue/background`.
///
/// `schedulePeriodic` submits a `BGAppRefreshTask` request; when the OS grants the
/// window, the registered handler invokes the Dart `runFlush` (the offline queue's
/// idempotent drain, S5-R38) and re-schedules. `cancel` removes pending requests.
///
/// NO business logic lives here (S5-R1). NOTE (native-unverified-here): code-complete
/// for the iOS toolchain; NOT compiled/run on the Linux CI host. An XCTest accompanies it.
final class BackgroundHandler: NSObject {

    /// Mirrors Android's `FlushWorker.UNIQUE_WORK` ("qcue.flush.periodic") so the
    /// flush job carries the same canonical identifier on both platforms. MUST match
    /// `BGTaskSchedulerPermittedIdentifiers` in Runner/Info.plist.
    static let taskIdentifier = "qcue.flush.periodic"
    // MUST be a strong ref: the BGTask fires `handleLaunch` minutes after launch and
    // sends an OUTBOUND `runFlush` on this channel. Flutter's messenger does not retain
    // a FlutterMethodChannel, so a `weak` ref here is deallocated right after `register`
    // returns → the invoke silently no-ops and nothing drains. The handler closure below
    // captures `[weak handler]`, so a strong channel does not create a retain cycle.
    private var channel: FlutterMethodChannel?

    static func register(messenger: FlutterBinaryMessenger) -> BackgroundHandler {
        let handler = BackgroundHandler()
        let method = FlutterMethodChannel(name: QcueChannels.background, binaryMessenger: messenger)
        handler.channel = method
        method.setMethodCallHandler { [weak handler] call, result in
            handler?.handle(call, result: result)
        }
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
        case "schedulePeriodic":
            schedule()
            result(nil)
        case "cancel":
            if #available(iOS 13.0, *) {
                BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: BackgroundHandler.taskIdentifier)
            }
            result(nil)
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    private func schedule() {
        guard #available(iOS 13.0, *) else { return }
        let request = BGAppRefreshTaskRequest(identifier: BackgroundHandler.taskIdentifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60)
        // Submitting again replaces the existing request → idempotent (S5-R37).
        try? BGTaskScheduler.shared.submit(request)
    }

    /// Invoked by the OS when it grants the background-refresh window. The launch
    /// handler is registered in `AppDelegate.didFinishLaunchingWithOptions` (before
    /// launch completes) and forwards here. Chains the next window, then runs the
    /// Dart-side idempotent drain (the offline queue dedupes on client id, S5-R38).
    @available(iOS 13.0, *)
    func handleLaunch(_ task: BGAppRefreshTask) {
        schedule() // chain the next window
        channel?.invokeMethod("runFlush", arguments: nil) { _ in
            task.setTaskCompleted(success: true)
        }
        task.expirationHandler = { task.setTaskCompleted(success: false) }
    }
}
