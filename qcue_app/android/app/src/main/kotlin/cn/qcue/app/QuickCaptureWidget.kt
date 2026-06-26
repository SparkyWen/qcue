package cn.qcue.app

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.widget.RemoteViews

/**
 * QCue S5-R45/R46 — the Android App Widget provider for quick capture.
 *
 * Renders ONLY a static affordance + the non-sensitive today-count (S5-R46), read
 * from the shared prefs the [WidgetPlugin] writes. Two taps:
 *   - Compose: a deep-link Intent (`qcue://capture/compose`) that launches the app
 *     at the always-ready capture field (S5-R45).
 *   - Quick capture: a broadcast to this provider that re-enters the app to
 *     enqueue a capture (offline-safe via the local queue) — handled by the app's
 *     widget event channel rather than persisting in the widget process.
 *
 * NO business logic lives here (S5-R1). NOTE (native-unverified-here): code-complete
 * for the Android toolchain; NOT compiled/run on the Linux CI host.
 */
class QuickCaptureWidget : AppWidgetProvider() {

    override fun onUpdate(
        context: Context,
        appWidgetManager: AppWidgetManager,
        appWidgetIds: IntArray,
    ) {
        val count = context.getSharedPreferences(WidgetPlugin.STORE, Context.MODE_PRIVATE)
            .getInt(WidgetPlugin.KEY_COUNT, 0)

        for (id in appWidgetIds) {
            val views = RemoteViews(context.packageName, R.layout.qcue_widget)
            views.setTextViewText(R.id.qcue_widget_count, "$count captured today")

            // Compose → deep-link the app to the capture field (S5-R45).
            val composeIntent = Intent(
                Intent.ACTION_VIEW, Uri.parse("qcue://capture/compose"),
            ).setPackage(context.packageName)
            val composePending = PendingIntent.getActivity(
                context, 0, composeIntent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            views.setOnClickPendingIntent(R.id.qcue_widget_compose, composePending)
            // Tapping the count or any background/padding area also composes, matching
            // iOS systemSmall's whole-widget `.widgetURL(qcue://capture/compose)`
            // fallback (the dedicated Quick button below keeps its own action).
            views.setOnClickPendingIntent(R.id.qcue_widget_root, composePending)
            views.setOnClickPendingIntent(R.id.qcue_widget_count, composePending)

            // Quick capture → broadcast back to this provider (offline-safe enqueue).
            val quickIntent = Intent(context, QuickCaptureWidget::class.java)
                .setAction(ACTION_QUICK_CAPTURE)
            views.setOnClickPendingIntent(
                R.id.qcue_widget_quick,
                PendingIntent.getBroadcast(
                    context, 1, quickIntent,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                ),
            )
            appWidgetManager.updateAppWidget(id, views)
        }
    }

    override fun onReceive(context: Context, intent: Intent) {
        super.onReceive(context, intent)
        if (intent.action == ACTION_QUICK_CAPTURE) {
            // Re-enter the app so the offline queue enqueues the capture; the app's
            // widget event channel emits {action: 'quickCapture'} on resume.
            val launch = Intent(
                Intent.ACTION_VIEW, Uri.parse("qcue://widget/quickCapture"),
            ).setPackage(context.packageName)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(launch)
        }
    }

    companion object {
        const val ACTION_QUICK_CAPTURE = "cn.qcue.WIDGET_QUICK_CAPTURE"
    }
}
