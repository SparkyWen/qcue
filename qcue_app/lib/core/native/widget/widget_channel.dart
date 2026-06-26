// QCue S5-R45/R46/R47: the home-screen quick-capture widget facade.
//
// Control flows over MethodChannel('qcue/widget'); widget-tap intents arrive over
// EventChannel('qcue/widget/events'). The facade does two things:
//   (a) refresh(): write a NON-SENSITIVE today-capture count to the shared
//       container + ask the OS to reload the widget timeline (S5-R46/R47). The
//       widget renders only the count + a static affordance — never idea/wiki
//       bodies (home/lock-screen privacy).
//   (b) start(): handle a widget tap — `compose` DEEP-LINKS into the Capture
//       screen's text field; `quickCapture` BACKGROUND-ENQUEUES a capture
//       (offline-safe, `origin='capture:widget'`) through the injected [Enqueue]
//       seam, without a full app launch (S5-R45).
//
//   - Native Android: an App Widget provider whose tap fires a deep-link Intent
//     (compose) or a broadcast that enqueues (quickCapture).
//   - Native iOS: a WidgetKit widget with `widgetURL` (compose) + an App Intent
//     (quickCapture). The enqueue uses the local queue, so it is offline-safe.
import 'dart:async';

import 'package:flutter/services.dart';
import '../channels.dart';
import '../protocol/capture_enqueue.dart';
import '../protocol/native_dtos.dart';

/// Called with the go_router location a widget tap deep-links to (S5-R45).
typedef DeepLink = void Function(String route);

class WidgetChannel {
  WidgetChannel({
    required Enqueue enqueue,
    required DeepLink onDeepLink,
    this._method = const MethodChannel(QcueChannels.widget),
    this._eventChannel = const EventChannel(QcueChannels.widgetEvents),
  })  :
        // ignore: prefer_initializing_formals — keep the readable named params
        _enqueue = enqueue,
        // ignore: prefer_initializing_formals
        _onDeepLink = onDeepLink;

  final Enqueue _enqueue;
  final DeepLink _onDeepLink;
  final MethodChannel _method;
  final EventChannel _eventChannel;
  StreamSubscription<dynamic>? _sub;

  /// The route a `compose` widget tap opens (the always-ready capture field).
  static const composeRoute = '/capture/compose';

  /// S5-R46/R47: publish the non-sensitive today-count to the widget, then ask
  /// the OS to reload its timeline so the count stays current after a capture or
  /// a Dream. NO idea/body content is ever sent — only the count.
  Future<void> refresh({required int todayCount}) async {
    await _method.invokeMethod<void>(
      'setCount',
      QcueChannels.envelope({'count': todayCount}),
    );
    await _method.invokeMethod<void>(
      'reloadTimelines',
      QcueChannels.envelope(),
    );
  }

  /// Begin handling widget-tap intents.
  void start() {
    _sub ??= _eventChannel.receiveBroadcastStream().listen(
      (raw) {
        if (raw is Map) unawaited(_onTap(raw));
      },
      onError: (Object _, StackTrace __) {/* native error envelope: ignore */},
    );
  }

  Future<void> _onTap(Map<dynamic, dynamic> raw) async {
    switch (raw['action'] as String?) {
      case 'compose':
        _onDeepLink(composeRoute);
        return;
      case 'quickCapture':
        final args = (raw['args'] as Map?)?.cast<String, dynamic>() ?? const {};
        final body = args['body'] as String? ?? '';
        await _enqueue(CaptureEnqueueReq(
          captureId: mintCaptureId(),
          body: body,
          origin: 'capture:widget',
        ));
        return;
      default:
        return; // unknown action: ignore (forward-compat)
    }
  }

  Future<void> dispose() async {
    await _sub?.cancel();
    _sub = null;
  }
}
