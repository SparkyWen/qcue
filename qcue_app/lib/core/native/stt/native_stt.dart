// QCue S5-R18/R19/R21: the on-device STT facade. Control flows over
// MethodChannel('qcue/stt') (isAvailable / requestPermission / start(locale) /
// stop); partial + final transcripts, errors and an availability signal stream
// in over EventChannel('qcue/stt/events'). Partials are DISPLAY-ONLY (S5-R19);
// the canonical body is the assembled SttFinal.transcript. The native side owns
// the mic + the OS recognizer; this is the thin Dart marshaling layer.
import 'dart:async';

import 'package:flutter/services.dart';
import '../channels.dart';

/// The mic+speech permission outcome (S5-R18/R30).
enum SttPermission { granted, denied, restricted, notDetermined }

SttPermission _permFrom(String? s) {
  switch (s) {
    case 'granted':
      return SttPermission.granted;
    case 'denied':
      return SttPermission.denied;
    case 'restricted':
      return SttPermission.restricted;
    default:
      return SttPermission.notDetermined;
  }
}

/// The closed STT error set carried on the event channel (S5-R21 mapping).
enum SttErrorKind {
  unavailable,
  unsupportedLocale,
  noSpeech,
  network,
  permission,
  cancelled,
  osError,
}

SttErrorKind _errKindFrom(String? s) {
  switch (s) {
    case 'unavailable':
      return SttErrorKind.unavailable;
    case 'unsupportedLocale':
      return SttErrorKind.unsupportedLocale;
    case 'noSpeech':
      return SttErrorKind.noSpeech;
    case 'network':
      return SttErrorKind.network;
    case 'permission':
      return SttErrorKind.permission;
    case 'cancelled':
      return SttErrorKind.cancelled;
    default:
      return SttErrorKind.osError;
  }
}

/// The assembled final result for one capture (delta→completed on the native
/// side; the harness only ever sees this, never a partial — S5-R19).
class SttFinalResult {
  const SttFinalResult({
    required this.captureId,
    required this.transcript,
    required this.onDevice,
    required this.localeTag,
    required this.audioMillis,
    this.confidence,
  });
  final String captureId;
  final String transcript;
  final bool onDevice;
  final String localeTag;
  final int audioMillis;
  final double? confidence;

  factory SttFinalResult.fromMap(Map<dynamic, dynamic> m) => SttFinalResult(
        captureId: m['captureId'] as String? ?? '',
        transcript: m['transcript'] as String? ?? '',
        onDevice: m['onDevice'] as bool? ?? false,
        localeTag: m['localeTag'] as String? ?? '',
        audioMillis: (m['audioMillis'] as num?)?.toInt() ?? 0,
        confidence: (m['confidence'] as num?)?.toDouble(),
      );
}

/// The event-channel taxonomy (S5-R19).
sealed class SttEvent {
  const SttEvent();
}

class SttPartial extends SttEvent {
  const SttPartial(this.captureId, this.text);
  final String captureId;
  final String text;
}

class SttFinal extends SttEvent {
  const SttFinal(this.result);
  final SttFinalResult result;
  String get transcript => result.transcript;
  bool get onDevice => result.onDevice;
  double? get confidence => result.confidence;
  int get audioMillis => result.audioMillis;
}

class SttError extends SttEvent {
  const SttError(this.captureId, this.kind, this.message);
  final String captureId;
  final SttErrorKind kind;
  final String? message;
}

class SttAvail extends SttEvent {
  const SttAvail(this.onDeviceAvailable, this.supportedLocales);
  final bool onDeviceAvailable;
  final List<String> supportedLocales;
}

/// The narrow surface the [NativeVoiceCaptureController] depends on — so it can
/// be faked in tests without a device.
abstract interface class SttFacade {
  Future<bool> isAvailable({String? locale});
  Future<SttPermission> requestPermission();
  Future<void> start({String? locale, required String captureId});
  Future<void> stop({String? captureId});
  Stream<SttEvent> get events;
}

/// The real platform STT facade. Stateless apart from the channels; safe as a
/// `const` provider value.
class NativeStt implements SttFacade {
  const NativeStt({
    this._method = const MethodChannel(QcueChannels.stt),
    this._eventChannel = const EventChannel(QcueChannels.sttEvents),
  });

  final MethodChannel _method;
  final EventChannel _eventChannel;

  @override
  Future<bool> isAvailable({String? locale}) async {
    final r = await _method.invokeMethod<bool>(
      'isAvailable',
      QcueChannels.envelope({if (locale != null) 'localeTag': locale}),
    );
    return r ?? false;
  }

  @override
  Future<SttPermission> requestPermission() async {
    final r = await _method.invokeMethod<String>(
      'requestPermission',
      QcueChannels.envelope(),
    );
    return _permFrom(r);
  }

  @override
  Future<void> start({String? locale, required String captureId}) async {
    await _method.invokeMethod<void>(
      'start',
      QcueChannels.envelope({
        'captureId': captureId,
        if (locale != null) 'localeTag': locale,
        'partialResults': true,
      }),
    );
  }

  @override
  Future<void> stop({String? captureId}) async {
    await _method.invokeMethod<void>(
      'stop',
      QcueChannels.envelope({if (captureId != null) 'captureId': captureId}),
    );
  }

  @override
  Stream<SttEvent> get events {
    late final StreamController<SttEvent> out;
    StreamSubscription<dynamic>? sub;
    out = StreamController<SttEvent>.broadcast(
      onListen: () {
        sub = _eventChannel.receiveBroadcastStream().listen(
          (raw) {
            final e = _decode(raw);
            if (e != null) out.add(e);
          },
          // A platform error envelope surfaces as a typed SttError, never as an
          // unhandled stream error (S5-R4 — no raw OS exception leaks).
          onError: (Object error, StackTrace _) {
            final ne = nativeErrorFrom(error);
            out.add(SttError('', _errFromNative(ne.kind), ne.message));
          },
        );
      },
      onCancel: () => sub?.cancel(),
    );
    return out.stream;
  }

  static SttErrorKind _errFromNative(NativeErrorKind k) {
    switch (k) {
      case NativeErrorKind.permissionDenied:
        return SttErrorKind.permission;
      case NativeErrorKind.unavailable:
        return SttErrorKind.unavailable;
      case NativeErrorKind.cancelled:
        return SttErrorKind.cancelled;
      case NativeErrorKind.rateLimited:
        return SttErrorKind.network;
      case NativeErrorKind.versionMismatch:
      case NativeErrorKind.osError:
        return SttErrorKind.osError;
    }
  }

  static SttEvent? _decode(dynamic raw) {
    if (raw is! Map) return null;
    switch (raw['event'] as String?) {
      case 'partial':
        return SttPartial(
            raw['captureId'] as String? ?? '', raw['text'] as String? ?? '');
      case 'final':
        // payload may be flat or nested under `result`.
        final src = raw['result'] is Map
            ? (raw['result'] as Map)
            : raw;
        return SttFinal(SttFinalResult.fromMap(src));
      case 'error':
        return SttError(
          raw['captureId'] as String? ?? '',
          _errKindFrom(raw['kind'] as String?),
          raw['message'] as String?,
        );
      case 'avail':
        final locales = (raw['supportedLocales'] as List?)
                ?.map((e) => e.toString())
                .toList() ??
            const <String>[];
        return SttAvail(raw['onDeviceAvailable'] as bool? ?? false, locales);
      default:
        return null;
    }
  }
}
