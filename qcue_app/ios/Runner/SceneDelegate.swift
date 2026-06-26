import Flutter
import UIKit

/// QCue S5 — forwards inbound `qcue://` deep links (widget compose / quickCapture)
/// to the AppDelegate channels and drains any Share-Extension-staged items when
/// the scene becomes active (S5-R42/R45).
///
/// NOTE (native-unverified-here): code-complete for the iOS toolchain; NOT compiled/run on Linux CI.
class SceneDelegate: FlutterSceneDelegate {

  override func scene(
    _ scene: UIScene,
    willConnectTo session: UISceneSession,
    options connectionOptions: UIScene.ConnectionOptions
  ) {
    super.scene(scene, willConnectTo: session, options: connectionOptions)
    for context in connectionOptions.urlContexts {
      AppDelegate.shared?.handleDeepLink(context.url)
    }
  }

  override func scene(_ scene: UIScene, openURLContexts URLContexts: Set<UIOpenURLContext>) {
    for context in URLContexts {
      AppDelegate.shared?.handleDeepLink(context.url)
    }
  }

  override func sceneDidBecomeActive(_ scene: UIScene) {
    super.sceneDidBecomeActive(scene)
    AppDelegate.shared?.drainPendingShares()
  }
}
