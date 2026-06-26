// QCue cloud-sync fix (Task 7): make an upload failure DEBUGGABLE instead of
// silent. The offline-first contract still holds — a capture is always queued
// locally and the app never throws in the user's face — but when the inner POST
// fails we now record WHY, so the UI can distinguish:
//   • unauthorized — not signed in / token rejected (the empty-Bearer 401 bug);
//   • network      — server unreachable / wrong Server URL / offline;
//   • other        — a non-401 server error (4xx/5xx).
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../net/ws_client.dart' show RpcError;

/// Why the most recent capture upload didn't reach the server.
enum SyncErrorReason { unauthorized, network, other }

/// A small, immutable snapshot of the last capture-upload outcome.
class SyncStatus {
  const SyncStatus({this.lastError, this.at});

  /// `null` ⇒ the last capture uploaded fine (or none has failed yet).
  final SyncErrorReason? lastError;

  /// When [lastError] was recorded (UTC), for a "last tried" surface.
  final DateTime? at;

  /// A clean (no-error) status.
  static const ok = SyncStatus();

  bool get hasError => lastError != null;

  /// A short human reason for the banner / debug surface.
  String? get message => switch (lastError) {
        SyncErrorReason.unauthorized =>
          "Not signed in — your captures aren't syncing. Sign in to upload.",
        SyncErrorReason.network =>
          "Can't reach the server — check the Server URL in Settings.",
        SyncErrorReason.other =>
          "The server rejected the upload — captures stay queued.",
        null => null,
      };

  /// Classify a thrown capture-upload error into a [SyncErrorReason].
  static SyncErrorReason classify(Object error) {
    if (error is RpcError) {
      if (error.isUnauthorized) return SyncErrorReason.unauthorized;
      // -32603/internal etc. that aren't transport are "other"; backpressure
      // is retried upstream and won't normally land here.
      return SyncErrorReason.other;
    }
    // A raw transport throw (SocketException, ClientException, TimeoutException)
    // surfaces as a plain Exception → treat as a network failure.
    return SyncErrorReason.network;
  }
}

class SyncStatusNotifier extends Notifier<SyncStatus> {
  @override
  SyncStatus build() => SyncStatus.ok;

  /// Record a failed upload reason (called from the offline capture catch).
  void recordError(SyncErrorReason reason) =>
      state = SyncStatus(lastError: reason, at: DateTime.now().toUtc());

  /// Clear the error after a successful upload.
  void recordSuccess() {
    if (state.hasError) state = SyncStatus.ok;
  }
}

/// The single source of truth for the last capture-upload outcome (Task 7).
final syncStatusProvider =
    NotifierProvider<SyncStatusNotifier, SyncStatus>(SyncStatusNotifier.new);
