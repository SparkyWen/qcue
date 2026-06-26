import Foundation

/// QCue S5-R42/R46 — the App Group container shared by the main app, the Share
/// Extension, and the WidgetKit widget. The extension STAGES shared items here
/// (it cannot host the Rust core), the app DRAINS them on launch/resume, and the
/// app writes the non-sensitive widget count here for the widget to render.
///
/// This file is compiled into ALL THREE targets (Runner, ShareExtension,
/// QcueWidget) so they agree on the keys + the App Group id.
///
/// NOTE (native-unverified-here): code-complete for the iOS toolchain; NOT compiled
/// or run on the Linux CI host (no Xcode).
/// One staged shared item: a string-keyed map (text|url + sourceApp).
public typealias StagedItem = [String: Any]

public enum SharedContainer {
    /// MUST match the App Group entitlement on every target.
    public static let appGroup = "group.cn.qcue.shared"
    private static let pendingKey = "qcue.share.pending"
    public static let countKey = "qcue.widget.count"

    private static var defaults: UserDefaults? {
        UserDefaults(suiteName: appGroup)
    }

    /// Stage one shared item (called by the Share Extension). Items are appended;
    /// the body is stored verbatim (S5-R43 — no transform).
    public static func stageSharedItem(_ item: StagedItem) {
        guard let d = defaults else { return }
        var list = d.array(forKey: pendingKey) as? [StagedItem] ?? []
        list.append(item)
        d.set(list, forKey: pendingKey)
    }

    /// Drain + clear the staged items (called by the app on launch/resume).
    public static func drainSharedItems() -> [StagedItem] {
        guard let d = defaults else { return [] }
        let list = d.array(forKey: pendingKey) as? [StagedItem] ?? []
        d.removeObject(forKey: pendingKey)
        return list
    }

    /// Write the non-sensitive today-count for the widget (S5-R46).
    public static func setWidgetCount(_ count: Int) {
        defaults?.set(count, forKey: countKey)
    }

    public static func widgetCount() -> Int {
        defaults?.integer(forKey: countKey) ?? 0
    }
}
