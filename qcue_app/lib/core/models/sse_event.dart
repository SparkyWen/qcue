// QCue S4-R24: the Clariose recall SSE taxonomy + the Dream taxonomy
// (Appendix A §3.4 / §4.1) as a sealed Dart type. Unknown kinds → UnknownEvent
// (forward-compat, S4-R23). Carries the canonical Citation{rel_path,start_line,
// end_line} contract (master §4 / Appendix A A-R25).

/// FFI/transport error envelope — never a raw panic (S4-R11).
enum FfiError { network, noKey, costCapped, cancelled, internal }

/// A citation as carried on the wire (master §4 Citation contract).
class Citation {
  const Citation({
    required this.relPath,
    required this.startLine,
    required this.endLine,
  });
  final String relPath;
  final int startLine;
  final int endLine;

  factory Citation.fromJson(Map<String, dynamic> j) => Citation(
        relPath: j['rel_path'] as String,
        startLine: j['start_line'] as int,
        endLine: j['end_line'] as int,
      );

  Map<String, dynamic> toJson() => {
        'rel_path': relPath,
        'start_line': startLine,
        'end_line': endLine,
      };

  /// Human label, e.g. `source.md:42`.
  String get label => startLine == endLine
      ? '$relPath:$startLine'
      : '$relPath:$startLine-$endLine';

  @override
  bool operator ==(Object other) =>
      other is Citation &&
      other.relPath == relPath &&
      other.startLine == startLine &&
      other.endLine == endLine;

  @override
  int get hashCode => Object.hash(relPath, startLine, endLine);
}

sealed class SseEvent {
  const SseEvent();

  factory SseEvent.fromJson(Map<String, dynamic> j) {
    final p = (j['payload'] as Map?)?.cast<String, dynamic>() ?? const {};
    // The recall driver emits `message_delta{delta}`, `tool_call{tool}`,
    // `usage{input,output,reasoning}`; older/test frames use `text`/`name`/
    // `*_tokens`. Accept BOTH spellings so the same sealed taxonomy serves the
    // real wire and the unit fixtures (forward-compat, never break old frames).
    int n(String a, String b) => (p[a] as num?)?.toInt() ?? (p[b] as num?)?.toInt() ?? 0;
    return switch (j['event'] as String?) {
      'session_started' =>
        SessionStarted(p['thread_id'] as String? ?? j['thread_id'] as String? ?? ''),
      'message_delta' =>
        MessageDelta(p['delta'] as String? ?? p['text'] as String? ?? ''),
      'tool_call' => ToolCall(p['tool'] as String? ?? p['name'] as String? ?? ''),
      'tool_result' =>
        ToolResult(p['tool'] as String? ?? p['name'] as String? ?? ''),
      'citation' => CitationEvent(Citation.fromJson(p)),
      'usage' => UsageEvent(
          inputTokens: n('input', 'input_tokens'),
          outputTokens: n('output', 'output_tokens'),
          reasoningTokens: n('reasoning', 'reasoning_tokens'),
        ),
      'reasoning' =>
        ReasoningDelta(p['delta'] as String? ?? p['text'] as String? ?? ''),
      'done' => const DoneEvent(),
      'error' =>
        ErrorEvent(p['message'] as String? ?? 'error', code: p['code'] as int?),
      'dream_started' => DreamStarted(
          jobId: p['job_id'] as String? ?? '',
          sessions: p['sessions'] as int? ?? 0,
        ),
      'progress' => DreamProgress(
          text: p['text'] as String? ?? '',
          toolUseCount: p['tool_use_count'] as int? ?? 0,
          pagesTouched: (p['pages_touched'] as List?)?.cast<String>() ?? const [],
        ),
      // The dream JobHandler emits `dream_completed{improved_pages}` /
      // `dream_failed{error}`; accept those alongside the bare `completed`/
      // `failed`/`pages_improved` spellings used by the UI fixtures.
      'completed' || 'dream_completed' => DreamCompleted(
          pagesImproved:
              (p['pages_improved'] as num?)?.toInt() ?? (p['improved_pages'] as num?)?.toInt() ?? 0,
        ),
      'failed' || 'dream_failed' =>
        DreamFailed(p['reason'] as String? ?? p['error'] as String? ?? 'failed'),
      _ => UnknownEvent(j['event'] as String? ?? 'unknown'),
    };
  }
}

// ── recall taxonomy ──
class SessionStarted extends SseEvent {
  const SessionStarted(this.threadId);
  final String threadId;
}

class MessageDelta extends SseEvent {
  const MessageDelta(this.text);
  final String text;
}

class ReasoningDelta extends SseEvent {
  const ReasoningDelta(this.text);
  final String text;
}

class ToolCall extends SseEvent {
  const ToolCall(this.name);
  final String name;
}

class ToolResult extends SseEvent {
  const ToolResult(this.name);
  final String name;
}

class CitationEvent extends SseEvent {
  const CitationEvent(this.citation);
  final Citation citation;
}

class UsageEvent extends SseEvent {
  const UsageEvent({
    required this.inputTokens,
    required this.outputTokens,
    required this.reasoningTokens,
  });
  final int inputTokens;
  final int outputTokens;
  final int reasoningTokens;
}

class DoneEvent extends SseEvent {
  const DoneEvent();
}

class ErrorEvent extends SseEvent {
  const ErrorEvent(this.message, {this.code});
  final String message;
  final int? code;
}

// ── dream taxonomy (Appendix A §4.1) ──
class DreamStarted extends SseEvent {
  const DreamStarted({required this.jobId, required this.sessions});
  final String jobId;
  final int sessions;
}

class DreamProgress extends SseEvent {
  const DreamProgress({
    required this.text,
    required this.toolUseCount,
    required this.pagesTouched,
  });
  final String text;
  final int toolUseCount;
  final List<String> pagesTouched;
}

class DreamCompleted extends SseEvent {
  const DreamCompleted({required this.pagesImproved});
  final int pagesImproved;
}

class DreamFailed extends SseEvent {
  const DreamFailed(this.reason);
  final String reason;
}

// ── forward-compat ──
class UnknownEvent extends SseEvent {
  const UnknownEvent(this.kind);
  final String kind;
}
