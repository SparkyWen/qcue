// QCue S5-R3/R4: the platform-channel namespace + versioning + the closed
// typed-error mapping. The Dart facades (STT, secure storage) and the native
// Kotlin/Swift handlers agree on these channel names and the single
// `schemaVersion` field carried on every method-channel payload. A native
// PlatformException(details:{kind,retryable}) maps to a closed NativeError set;
// an unexpected OS exception is wrapped as `osError` and never leaked raw.
import 'package:flutter/services.dart';

/// Channel-name + version constants — the binding contract with native code.
class QcueChannels {
  const QcueChannels._();

  /// Bumped on any breaking change to a channel payload (S5-R3). Native rejects
  /// an unknown major with a `versionMismatch` error rather than mis-parsing.
  static const int schemaVersion = 1;

  // ── On-device STT (Speech / SpeechRecognizer) ──
  static const String stt = 'qcue/stt';
  static const String sttEvents = 'qcue/stt/events';

  // ── Secure key storage (Keychain / Keystore + biometric) ──
  static const String secure = 'qcue/secure';

  // ── Share-sheet ingest (intent filter / Share Extension → App Group) ──
  static const String share = 'qcue/share';
  static const String shareEvents = 'qcue/share/events';

  // ── Home-screen quick-capture widget (App Widget / WidgetKit) ──
  static const String widget = 'qcue/widget';
  static const String widgetEvents = 'qcue/widget/events';

  // ── Local notifications (NotificationManager / UNUserNotificationCenter) ──
  static const String notif = 'qcue/notif';
  static const String notifEvents = 'qcue/notif/events';

  // ── Background flush scheduler (WorkManager / BGTaskScheduler) ──
  static const String background = 'qcue/background';

  /// Helper: the base payload every method call carries (the version guard).
  static Map<String, dynamic> envelope([Map<String, dynamic>? extra]) => {
        'schemaVersion': schemaVersion,
        if (extra != null) ...extra,
      };
}

/// The closed error-kind set carried in PlatformException.details.kind (S5-R4).
enum NativeErrorKind {
  permissionDenied,
  unavailable,
  cancelled,
  osError,
  versionMismatch,
  rateLimited,
}

const _nativeErrorWire = <String, NativeErrorKind>{
  'permissionDenied': NativeErrorKind.permissionDenied,
  'unavailable': NativeErrorKind.unavailable,
  'cancelled': NativeErrorKind.cancelled,
  'osError': NativeErrorKind.osError,
  'versionMismatch': NativeErrorKind.versionMismatch,
  'rateLimited': NativeErrorKind.rateLimited,
};

/// A typed, Dart-side native error. Never carries key/secret material.
class NativeError implements Exception {
  const NativeError(this.kind, {this.message, this.retryable = false});
  final NativeErrorKind kind;
  final String? message;
  final bool retryable;

  @override
  String toString() => 'NativeError(${kind.name}, retryable: $retryable)';
}

/// Map any thrown error from a method channel to the closed [NativeError] set.
/// An unrecognized `kind` or a non-PlatformException degrades to `osError`
/// (S5-R4) — the raw OS exception is never surfaced.
NativeError nativeErrorFrom(Object error) {
  if (error is PlatformException) {
    final details = error.details;
    String? kindStr = error.code;
    bool retryable = false;
    if (details is Map) {
      final m = details.cast<dynamic, dynamic>();
      kindStr = (m['kind'] as String?) ?? kindStr;
      retryable = m['retryable'] == true;
    }
    final kind = _nativeErrorWire[kindStr] ?? NativeErrorKind.osError;
    return NativeError(kind, message: error.message, retryable: retryable);
  }
  return NativeError(NativeErrorKind.osError, message: error.toString());
}
