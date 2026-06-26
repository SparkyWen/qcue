// QCue S5-R5/R8/R10/R33/R34: the platform-channel DTOs the native Kotlin/Swift
// side mirrors. This is the Dart-side codegen-source-of-truth for the share /
// notification / capture shapes that cross `qcue/share`, `qcue/notif` and that
// every native capture path enqueues through. Two cardinal rules live here:
//   - a SharedItem records its `origin` (`share:<kind>:<source>`) faithfully and
//     captures its body VERBATIM — S5 never interprets shared text as
//     instructions (S5-R10/R43); S2 fences it downstream.
//   - QNotifKind is a CLOSED enum; an unknown push/notif kind is dropped, not
//     coerced (S5-R33); each kind maps to exactly one go_router route (S5-R34).
import '../channels.dart';

/// One inbound shared item handed over `qcue/share/events` when the OS gives the
/// app content from the share sheet (iOS Share Extension via App Group; Android
/// `ACTION_SEND` intent). text / url / file(imageRef), tagged with its source.
class SharedItem {
  const SharedItem({this.text, this.url, this.imageRef, required this.sourceApp});

  final String? text;
  final String? url;
  final String? imageRef;
  final String sourceApp;

  /// Upper bound on a captured share body (mirrors the iOS Share Extension's
  /// `ShareViewController.maxTextChars`). SECURITY: the iOS drain reads from the
  /// `group.cn.qcue.shared` App Group container, a trust boundary writable by the
  /// app's own extensions; the writer's cap is NOT trustworthy on the read side,
  /// so we re-clamp here so a tampered/oversized staged item can't be ingested.
  static const maxBodyChars = 100000;

  /// Defensively coerce a channel value to a non-empty [String]; anything else
  /// (a non-string from a malformed staged payload, or empty) becomes `null`
  /// instead of throwing a `CastError` mid-drain. Replaces a raw `as String?`,
  /// which throws on a non-null non-String value.
  static String? _str(Object? v) => (v is String && v.isNotEmpty) ? v : null;

  factory SharedItem.fromMap(Map<dynamic, dynamic> m) => SharedItem(
        text: _str(m['text']),
        url: _str(m['url']),
        imageRef: _str(m['imageRef']),
        sourceApp: _str(m['sourceApp']) ?? 'unknown',
      );

  /// The capture body: the URL takes precedence (a web clip), else the text,
  /// else the image ref. Verbatim — never transformed (S5-R43) — but length-
  /// clamped to [maxBodyChars] as untrusted-input defense. `null` ⇒ nothing to
  /// capture (an empty/unsupported share is drained and ignored).
  String? get captureBody {
    final body = (url != null && url!.isNotEmpty)
        ? url
        : (text != null && text!.isNotEmpty)
            ? text
            : (imageRef != null && imageRef!.isNotEmpty)
                ? imageRef
                : null;
    if (body == null) return null;
    return body.length > maxBodyChars ? body.substring(0, maxBodyChars) : body;
  }

  /// The `origin` recorded on the capture for S2 untrusted-fencing (S5-R10):
  /// `share:<kind>:<sourceApp>` where kind ∈ {web (URL), text, image}. The
  /// downstream ingest fences the body; S5 only records the provenance.
  String get captureOrigin {
    final kind = (url != null && url!.isNotEmpty)
        ? 'web'
        : (text != null && text!.isNotEmpty)
            ? 'text'
            : 'image';
    return 'share:$kind:$sourceApp';
  }
}

/// The three QCue notification kinds (S5-R33, closed). An unknown kind from a
/// push/notif payload is dropped, never displayed.
enum QNotifKind { dreamComplete, ingestNeedsReview, syncConflict }

const _notifKindWire = <String, QNotifKind>{
  'dreamComplete': QNotifKind.dreamComplete,
  'ingestNeedsReview': QNotifKind.ingestNeedsReview,
  'syncConflict': QNotifKind.syncConflict,
};
const _notifKindToWire = <QNotifKind, String>{
  QNotifKind.dreamComplete: 'dreamComplete',
  QNotifKind.ingestNeedsReview: 'ingestNeedsReview',
  QNotifKind.syncConflict: 'syncConflict',
};

/// Parse a notif-kind wire token; `null` for an unknown kind (S5-R33 drop).
QNotifKind? qNotifKindFromWire(String? s) => s == null ? null : _notifKindWire[s];
String qNotifKindToWire(QNotifKind k) => _notifKindToWire[k]!;

/// S5-R34: every notification deep-links to exactly one go_router route. The
/// route map carries the S3 entity id (`jobs.id` for dream, etc.). Tapping the
/// same notification twice navigates once (idempotent — go_router de-dupes).
String deepLinkRouteFor(QNotifKind kind, Map<String, String> route) {
  // v0.2.2: Activity lives under Settings now (`/settings/activity`), so every
  // notification deep-link targets that prefix.
  switch (kind) {
    case QNotifKind.dreamComplete:
      final id = route['id'];
      return (id != null && id.isNotEmpty)
          ? '/settings/activity/dream/$id'
          : '/settings/activity';
    case QNotifKind.ingestNeedsReview:
      // Activity → ingest review (the Approval Center, Master §7).
      return '/settings/activity';
    case QNotifKind.syncConflict:
      // Activity → conflict resolver (Master §7 sync).
      return '/settings/activity';
  }
}

/// A local notification to show through `qcue/notif`. The factories encode the
/// three honest titles; the count for dreamComplete is the SERVER's number,
/// passed in verbatim, never recomputed on device (S5-R36).
class LocalNotif {
  const LocalNotif({
    required this.kind,
    required this.title,
    required this.body,
    this.route = const {},
  });

  final QNotifKind kind;
  final String title;
  final String body;
  final Map<String, String> route;

  /// "QCue improved N pages" — N is the Dream `jobs.result.filesUpdated` (S5-R36).
  factory LocalNotif.dreamComplete({required int pages, required String jobId}) {
    final unit = pages == 1 ? 'page' : 'pages';
    return LocalNotif(
      kind: QNotifKind.dreamComplete,
      title: 'QCue improved $pages $unit',
      body: 'Open Activity to see what changed.',
      route: {'id': jobId},
    );
  }

  factory LocalNotif.ingestNeedsReview({required int count}) {
    final unit = count == 1 ? 'capture needs' : 'captures need';
    return LocalNotif(
      kind: QNotifKind.ingestNeedsReview,
      title: 'QCue: $count $unit review',
      body: 'Review your recent captures in Activity.',
    );
  }

  factory LocalNotif.syncConflict() => const LocalNotif(
        kind: QNotifKind.syncConflict,
        title: 'QCue: a sync conflict needs your choice',
        body: 'Resolve the conflict in Activity.',
      );

  /// The deep-link route this notification navigates to on tap (S5-R34).
  String get deepLink => deepLinkRouteFor(kind, route);

  /// The channel payload (the `schemaVersion` guard is carried on every map).
  Map<String, dynamic> toMap() => QcueChannels.envelope({
        'kind': qNotifKindToWire(kind),
        'title': title,
        'body': body,
        'route': route,
      });
}

/// The shape every native capture path enqueues — a client-minted id (uuidv7) +
/// the body + the `origin` recorded for S2 fencing (S5-R8/R10). The actual
/// persistence is the offline `IdeaCache.enqueueCapture`; this is the value
/// object the facades hand to the (injected) enqueue callback.
class CaptureEnqueueReq {
  const CaptureEnqueueReq({
    required this.captureId,
    required this.body,
    required this.origin,
  });

  final String captureId;
  final String body;
  final String origin;
}
