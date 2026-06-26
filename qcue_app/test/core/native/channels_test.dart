// QCue S5-R3/R4: the channel namespace + versioning + the typed-error mapping.
// Channel names are stable constants; every payload carries a schemaVersion; a
// PlatformException(details:{kind,retryable}) maps to a closed NativeError enum;
// an unexpected/unknown OS exception is wrapped as osError, never leaked raw.
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/native/channels.dart';

void main() {
  test('S5-R3: channel names + schema version are the stable contract', () {
    expect(QcueChannels.stt, 'qcue/stt');
    expect(QcueChannels.sttEvents, 'qcue/stt/events');
    expect(QcueChannels.secure, 'qcue/secure');
    expect(QcueChannels.schemaVersion, 1);
  });

  test('S5-R4: PlatformException maps to the closed NativeError kinds', () {
    NativeError map(String code) => nativeErrorFrom(
          PlatformException(code: code, message: 'm', details: {'kind': code}),
        );
    expect(map('permissionDenied').kind, NativeErrorKind.permissionDenied);
    expect(map('unavailable').kind, NativeErrorKind.unavailable);
    expect(map('cancelled').kind, NativeErrorKind.cancelled);
    expect(map('versionMismatch').kind, NativeErrorKind.versionMismatch);
    expect(map('rateLimited').kind, NativeErrorKind.rateLimited);
    expect(map('osError').kind, NativeErrorKind.osError);
  });

  test('S5-R4: an unknown kind / raw exception degrades to osError', () {
    final e = nativeErrorFrom(
        PlatformException(code: 'totally-unknown', message: 'boom'));
    expect(e.kind, NativeErrorKind.osError);
    // a non-PlatformException is also wrapped, never leaked raw.
    final e2 = nativeErrorFrom(StateError('weird'));
    expect(e2.kind, NativeErrorKind.osError);
  });

  test('S5-R4: retryable flag is carried from details', () {
    final e = nativeErrorFrom(PlatformException(
        code: 'rateLimited',
        message: 'slow down',
        details: {'kind': 'rateLimited', 'retryable': true}));
    expect(e.kind, NativeErrorKind.rateLimited);
    expect(e.retryable, isTrue);
  });
}
