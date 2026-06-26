// QCue S5-R30/R33/R34/R36: the local-notification facade.
//
// Control flows over MethodChannel('qcue/notif'); notification taps arrive over
// EventChannel('qcue/notif/events'). The facade:
//   - request notification permission just-in-time (S5-R30);
//   - show() a [LocalNotif] for each of the three [QNotifKind]s — dreamComplete
//     ("Improved N pages", the server's count, S5-R36), ingestNeedsReview,
//     syncConflict — each carrying its deep-link route;
//   - on a TAP, deep-link to the right go_router route (S5-R34) via the injected
//     [DeepLink] callback; an unknown kind is dropped (S5-R33).
//
//   - Native Android: NotificationManager channels + a tap PendingIntent that
//     re-enters the app with the deep-link route.
//   - Native iOS: UNUserNotificationCenter + userNotificationCenter(didReceive:)
//     posting the route over the event channel.
//
// Push/FCM/APNs registration is roadmap (registerPushToken is a documented stub).
import 'dart:async';

import 'package:flutter/services.dart';
import '../channels.dart';
import '../protocol/native_dtos.dart';
import '../widget/widget_channel.dart' show DeepLink;

/// The notification-permission outcome (S5-R30). `provisional` is the iOS quiet
/// grant; treated as granted for show purposes.
enum NotifPermission { granted, denied, restricted, notDetermined, provisional }

NotifPermission _permFrom(String? s) {
  switch (s) {
    case 'granted':
      return NotifPermission.granted;
    case 'denied':
      return NotifPermission.denied;
    case 'restricted':
      return NotifPermission.restricted;
    case 'provisional':
      return NotifPermission.provisional;
    default:
      return NotifPermission.notDetermined;
  }
}

class NotifChannel {
  NotifChannel({
    required DeepLink onDeepLink,
    this._method = const MethodChannel(QcueChannels.notif),
    this._eventChannel = const EventChannel(QcueChannels.notifEvents),
  }) :
        // ignore: prefer_initializing_formals — keep the readable `onDeepLink:`
        _onDeepLink = onDeepLink;

  final DeepLink _onDeepLink;
  final MethodChannel _method;
  final EventChannel _eventChannel;
  StreamSubscription<dynamic>? _sub;

  /// Request OS notification permission (S5-R30, just-in-time).
  Future<NotifPermission> requestPermission() async {
    final r = await _method.invokeMethod<String>(
      'requestPermission',
      QcueChannels.envelope(),
    );
    return _permFrom(r);
  }

  /// Show a local notification (S5-R33). The map carries the kind, honest title,
  /// body and deep-link route + the `schemaVersion` guard.
  Future<void> show(LocalNotif n) async {
    await _method.invokeMethod<void>('show', n.toMap());
  }

  /// Clear any pending notifications of one kind.
  Future<void> cancelKind(QNotifKind kind) async {
    await _method.invokeMethod<void>(
      'cancelKind',
      QcueChannels.envelope({'kind': qNotifKindToWire(kind)}),
    );
  }

  /// ROADMAP (S5-R35): register the APNs/FCM push token with S3. Push delivery is
  /// not wired at M5 — this is the documented seam; locally-sourced notifications
  /// (Dream finished on-device, etc.) ship now via [show].
  Future<void> registerPushToken() async {
    await _method.invokeMethod<void>(
      'registerPushToken',
      QcueChannels.envelope(),
    );
  }

  /// Begin handling notification taps → deep-link routes (S5-R34).
  void start() {
    _sub ??= _eventChannel.receiveBroadcastStream().listen(
      (raw) {
        if (raw is! Map) return;
        final kind = qNotifKindFromWire(raw['kind'] as String?);
        if (kind == null) return; // unknown kind: dropped (S5-R33)
        final route = ((raw['route'] as Map?) ?? const {})
            .map((k, v) => MapEntry(k.toString(), v.toString()));
        _onDeepLink(deepLinkRouteFor(kind, route));
      },
      onError: (Object _, StackTrace __) {/* native error envelope: ignore */},
    );
  }

  Future<void> dispose() async {
    await _sub?.cancel();
    _sub = null;
  }
}
