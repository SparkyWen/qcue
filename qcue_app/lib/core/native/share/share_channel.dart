// QCue S5-R42/R43: the share-sheet ingest facade.
//
// Control + drain flow over MethodChannel('qcue/share'); inbound shared items
// arrive over EventChannel('qcue/share/events'). On receipt of a [SharedItem]
// the facade ENQUEUES a clip capture through the injected offline-safe [Enqueue]
// seam, with `origin='share:<kind>:<source>'` and the body captured VERBATIM
// (S5-R43 — S5 never interprets shared text as instructions; S2 fences it).
//
//   - Native Android: an `ACTION_SEND` intent filter on the MainActivity forwards
//     the shared text/url to the event channel.
//   - Native iOS: a Share Extension writes the item to the App Group container;
//     `drainPending()` reads it on launch/resume via the method channel.
//
// Both paths are offline-safe: the enqueue persists locally first and dedupes on
// retry (the offline `IdeaCache`). The Dart layer holds no device content state.
import 'dart:async';

import 'package:flutter/services.dart';
import '../channels.dart';
import '../protocol/capture_enqueue.dart';
import '../protocol/native_dtos.dart';

class ShareChannel {
  ShareChannel({
    required Future<void> Function(CaptureEnqueueReq req) enqueue,
    this._method = const MethodChannel(QcueChannels.share),
    this._eventChannel = const EventChannel(QcueChannels.shareEvents),
  }) :
        // ignore: prefer_initializing_formals — keep the readable `enqueue:` name
        _enqueue = enqueue;

  final Enqueue _enqueue;
  final MethodChannel _method;
  final EventChannel _eventChannel;
  StreamSubscription<dynamic>? _sub;

  /// Begin listening for live shared items (OS handed content while running).
  /// Each event enqueues a capture; an empty/unsupported item is ignored.
  void start() {
    _sub ??= _eventChannel.receiveBroadcastStream().listen(
      (raw) {
        if (raw is Map) {
          // Fire-and-forget: the enqueue is local-first, so it cannot block the
          // event loop; errors degrade silently (the item stays staged).
          unawaited(_ingest(SharedItem.fromMap(raw)));
        }
      },
      onError: (Object _, StackTrace __) {/* native error envelope: ignore */},
    );
  }

  /// Pull any items the Share Extension staged in the App Group while the app
  /// was killed (iOS), draining them into the capture queue on launch/resume.
  Future<void> drainPending() async {
    final List<dynamic>? items = await _method.invokeMethod<List<dynamic>>(
      'drainPending',
      QcueChannels.envelope(),
    );
    if (items == null) return;
    for (final raw in items) {
      if (raw is Map) await _ingest(SharedItem.fromMap(raw));
    }
  }

  Future<void> _ingest(SharedItem item) async {
    final body = item.captureBody;
    if (body == null) return; // nothing to capture (S5-R44 unsupported → drop)
    await _enqueue(CaptureEnqueueReq(
      captureId: mintCaptureId(),
      body: body,
      origin: item.captureOrigin,
    ));
  }

  Future<void> dispose() async {
    await _sub?.cancel();
    _sub = null;
  }
}
