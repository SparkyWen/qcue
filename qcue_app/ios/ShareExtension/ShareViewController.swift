import UIKit

/// QCue S5-R42/R43/R44 — the iOS Share Extension.
///
/// A SEPARATE OS process (the app may not be running). It extracts the shared
/// text / URL VERBATIM (S5-R43 — no transform), enforces a size cap (S5-R44), and
/// STAGES the item in the App Group container (`SharedContainer`). The main app
/// drains it on next launch/resume and enqueues a clip capture offline-safe — the
/// extension itself NEVER runs the Rust core or ingest (it's a constrained process).
///
/// This target needs: the App Group entitlement (group.cn.qcue.shared) and to
/// compile `ios/Shared/SharedContainer.swift`.
///
/// NOTE (native-unverified-here): code-complete for the iOS toolchain; NOT compiled/run on Linux CI.
final class ShareViewController: UIViewController {

    private let maxTextChars = 100_000
    // Public UTI strings — iOS 13-safe: avoids the iOS 14 `UTType` API and the
    // iOS 15-deprecated `kUTType*` constants, so share-in works on the app's full
    // floor (iOS 13+).
    private let urlUTI = "public.url"
    private let textUTI = "public.plain-text"

    override func viewDidLoad() {
        super.viewDidLoad()
        handleShare()
    }

    private func handleShare() {
        guard let item = (extensionContext?.inputItems.first as? NSExtensionItem),
              let providers = item.attachments else {
            return complete()
        }
        let group = DispatchGroup()
        for provider in providers {
            if provider.hasItemConformingToTypeIdentifier(urlUTI) {
                group.enter()
                provider.loadItem(forTypeIdentifier: urlUTI, options: nil) { [weak self] data, _ in
                    if let url = data as? URL { self?.stage(url: url.absoluteString) }
                    group.leave()
                }
            } else if provider.hasItemConformingToTypeIdentifier(textUTI) {
                group.enter()
                provider.loadItem(forTypeIdentifier: textUTI, options: nil) { [weak self] data, _ in
                    if let text = data as? String { self?.stage(text: text) }
                    group.leave()
                }
            }
        }
        group.notify(queue: .main) { [weak self] in self?.complete() }
    }

    private func sourceApp() -> String {
        // The host bundle id is not directly exposed; record a stable label.
        "ios-share"
    }

    private func stage(url: String) {
        guard url.count <= maxTextChars else { return } // S5-R44 cap
        SharedContainer.stageSharedItem(["url": url, "sourceApp": sourceApp()])
    }

    private func stage(text: String) {
        guard text.count <= maxTextChars else { return } // S5-R44 cap
        SharedContainer.stageSharedItem(["text": text, "sourceApp": sourceApp()])
    }

    private func complete() {
        extensionContext?.completeRequest(returningItems: [], completionHandler: nil)
    }
}
