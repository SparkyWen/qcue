// QCue D4: the typed failure the cloud STT path surfaces. Lives in core/models (not core/net) so the
// Capture field can catch it and show the reason WITHOUT importing the transport layer (layering law
// S4-R1). The api client throws it; the voice controller propagates it; the field renders [uiMessage].

/// Why a cloud transcription attempt failed (D4). The server returns a precise reason in its
/// `{success:false, error}` envelope; the app surfaces it instead of one opaque message.
enum TranscribeErrorKind { provider, network, noKey }

/// Thrown by `QcueApiClient.transcribe` when the server reports the transcription failed
/// (`success:false`) or the network is unreachable. [message] is the server's verbatim reason.
class TranscribeException implements Exception {
  const TranscribeException(this.message,
      {this.kind = TranscribeErrorKind.provider});
  final String message;
  final TranscribeErrorKind kind;

  /// A user-facing line. The no-key case is already actionable; a network failure gets a connection
  /// hint; a raw provider error is surfaced (trimmed) so the cause is never hidden again.
  String get uiMessage {
    switch (kind) {
      case TranscribeErrorKind.noKey:
        return message.isEmpty
            ? 'No OpenAI key set — add one in Settings to use voice.'
            : message;
      case TranscribeErrorKind.network:
        return "Couldn't reach the server to transcribe — check your connection.";
      case TranscribeErrorKind.provider:
        final m =
            message.length > 240 ? '${message.substring(0, 240)}…' : message;
        return 'Transcription failed: $m';
    }
  }

  @override
  String toString() => 'TranscribeException($kind, $message)';
}
