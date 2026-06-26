import BackgroundTasks
import Flutter
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  // QCue S5 — retained native capability handlers.
  private var sttHandler: SttHandler?
  private var secureHandler: SecureHandler?
  private var shareHandler: ShareHandler?
  private var widgetHandler: WidgetHandler?
  private var notifHandler: NotifHandler?
  private var backgroundHandler: BackgroundHandler?

  // Exposed so the SceneDelegate can forward inbound `qcue://` deep links + drain
  // staged shares on resume.
  static weak var shared: AppDelegate?

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    AppDelegate.shared = self
    // S5-R37 — register the background-flush BGTask launch handler BEFORE the app
    // finishes launching (Apple requires every BGTask launch handler to be registered
    // here, exactly once). The handler object is created later when the implicit
    // Flutter engine attaches; by the time the OS actually launches the task (≥15 min
    // out) `backgroundHandler` exists, so the closure forwards to it lazily.
    if #available(iOS 13.0, *) {
      BGTaskScheduler.shared.register(
        forTaskWithIdentifier: BackgroundHandler.taskIdentifier,
        using: nil
      ) { [weak self] task in
        guard let refresh = task as? BGAppRefreshTask else { return }
        self?.backgroundHandler?.handleLaunch(refresh)
      }
    }
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
    let messenger = engineBridge.pluginRegistry.registrar(forPlugin: "QcueNative")!.messenger()
    sttHandler = SttHandler.register(messenger: messenger)
    secureHandler = SecureHandler.register(messenger: messenger)
    shareHandler = ShareHandler.register(messenger: messenger)
    widgetHandler = WidgetHandler.register(messenger: messenger)
    notifHandler = NotifHandler.register(messenger: messenger)
    backgroundHandler = BackgroundHandler.register(messenger: messenger)
    // NOTE: the BGTask launch handler is registered once in
    // didFinishLaunchingWithOptions (above); registering again here would crash.
  }

  /// S5-R42 — drain any items the Share Extension staged while the app was killed.
  func drainPendingShares() {
    shareHandler?.emitPending()
  }

  /// S5-R45 — forward a `qcue://` widget/notification deep link to the right
  /// channel (compose / quickCapture).
  func handleDeepLink(_ url: URL) {
    guard url.scheme == "qcue" else { return }
    switch url.host {
    case "capture" where url.path == "/compose":
      widgetHandler?.deliverTap(action: "compose")
    case "widget" where url.path == "/quickCapture":
      widgetHandler?.deliverTap(action: "quickCapture")
    default:
      break
    }
  }
}
