import SwiftUI
import WidgetKit

/// QCue S5-R45/R46 — the iOS WidgetKit home-screen quick-capture widget.
///
/// Renders ONLY a static affordance + the non-sensitive today-count (S5-R46),
/// read from the App Group container the app writes. Two affordances:
///   - Compose: a `widgetURL(qcue://capture/compose)` deep-link into the app's
///     always-ready capture field (S5-R45).
///   - Quick capture: a second tappable region with `qcue://widget/quickCapture`,
///     which re-enters the app to enqueue offline-safe (the widget process never
///     persists). On iOS 17+ an App Intent could enqueue in-place; the deep-link
///     keeps the offline queue as the single capture sink (S5-R2).
///
/// This target needs the App Group entitlement (group.cn.qcue.shared) and to
/// compile `ios/Shared/SharedContainer.swift`.
///
/// NOTE (native-unverified-here): code-complete for the iOS toolchain; NOT compiled/run on Linux CI.

struct QcueEntry: TimelineEntry {
    let date: Date
    let count: Int
}

struct QcueProvider: TimelineProvider {
    func placeholder(in context: Context) -> QcueEntry {
        QcueEntry(date: Date(), count: 0)
    }

    func getSnapshot(in context: Context, completion: @escaping (QcueEntry) -> Void) {
        completion(QcueEntry(date: Date(), count: SharedContainer.widgetCount()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<QcueEntry>) -> Void) {
        // App-driven reload (S5-R47): a single entry, refreshed when the app calls
        // WidgetCenter.reloadAllTimelines — never a network poll in the widget.
        let entry = QcueEntry(date: Date(), count: SharedContainer.widgetCount())
        completion(Timeline(entries: [entry], policy: .never))
    }
}

struct QcueWidgetEntryView: View {
    var entry: QcueProvider.Entry

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("\(entry.count) captured today")
                .font(.caption)
                .foregroundColor(.secondary)
            HStack(spacing: 8) {
                Link(destination: URL(string: "qcue://capture/compose")!) {
                    Text("Compose")
                        .frame(maxWidth: .infinity)
                        .padding(6)
                        .background(Color.blue.opacity(0.2))
                        .cornerRadius(8)
                }
                Link(destination: URL(string: "qcue://widget/quickCapture")!) {
                    Text("Quick")
                        .frame(maxWidth: .infinity)
                        .padding(6)
                        .background(Color.green.opacity(0.2))
                        .cornerRadius(8)
                }
            }
        }
        .padding()
        // systemSmall ignores `Link`; a whole-widget tap deep-links to compose,
        // matching Android's QuickCaptureWidget single-tap → capture/compose.
        .widgetURL(URL(string: "qcue://capture/compose"))
    }
}

@main
struct QcueWidget: Widget {
    let kind = "QcueQuickCaptureWidget"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: kind, provider: QcueProvider()) { entry in
            QcueWidgetEntryView(entry: entry)
        }
        .configurationDisplayName("QCue Quick Capture")
        .description("Capture a new idea, or compose one — without opening the app.")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}
