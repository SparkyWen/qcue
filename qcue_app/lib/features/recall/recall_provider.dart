// QCue REC-R7: recall is now a multi-turn CONVERSATION. `ask()` APPENDS a turn (never replaces), the
// thread id is captured from SessionStarted and REUSED on the next ask (continue — the model sees prior
// turns). `openConversation()` loads a reopened thread's history; "new" resets to an empty conversation.
import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/sse_event.dart';
import '../../core/net/api_client_provider.dart';
import '../../core/sync/cache_revision.dart';
import 'recall_selection.dart';

/// One recall exchange: the question and its (streaming) answer. (Unchanged shape — the screen reads it.)
class RecallTurn {
  const RecallTurn({
    required this.question,
    this.answer = '',
    this.reasoning = '',
    this.citations = const [],
    this.streaming = true,
    this.error,
  });

  final String question;
  final String answer;
  final String reasoning;
  final List<Citation> citations;
  final bool streaming;
  final String? error;

  RecallTurn copyWith({
    String? answer,
    String? reasoning,
    List<Citation>? citations,
    bool? streaming,
    String? error,
  }) =>
      RecallTurn(
        question: question,
        answer: answer ?? this.answer,
        reasoning: reasoning ?? this.reasoning,
        citations: citations ?? this.citations,
        streaming: streaming ?? this.streaming,
        error: error ?? this.error,
      );
}

/// The whole conversation: an ordered list of turns + the (server-assigned) thread id once known.
class RecallConversation {
  const RecallConversation({this.threadId, this.title, this.turns = const []});

  final String? threadId;
  final String? title;
  final List<RecallTurn> turns;

  bool get streaming => turns.isNotEmpty && turns.last.streaming;

  RecallConversation copyWith({String? threadId, String? title, List<RecallTurn>? turns}) =>
      RecallConversation(
        threadId: threadId ?? this.threadId,
        title: title ?? this.title,
        turns: turns ?? this.turns,
      );

  /// Replace the last (in-flight) turn — used as the stream folds events in.
  RecallConversation withLastTurn(RecallTurn t) {
    final next = [...turns];
    next[next.length - 1] = t;
    return copyWith(turns: next);
  }
}

class RecallNotifier extends Notifier<RecallConversation?> {
  StreamSubscription<SseEvent>? _sub;

  @override
  RecallConversation? build() {
    ref.onDispose(() => _sub?.cancel());
    return null;
  }

  /// Start a brand-new conversation (the "＋ new" action / first load).
  void startNew() {
    _sub?.cancel();
    state = null;
  }

  /// Reopen a past conversation: load its prior turns and set it active so the next `ask` CONTINUES it.
  Future<void> openConversation(String threadId, {String? title}) async {
    _sub?.cancel();
    final prior = await ref.read(apiClientProvider).getConversationMessages(threadId);
    // fold the persisted (user,assistant) pairs into completed turns.
    final turns = <RecallTurn>[];
    for (final m in prior) {
      if (m.role == 'user') {
        turns.add(RecallTurn(question: m.content, streaming: false));
      } else if (m.role == 'assistant' && turns.isNotEmpty) {
        turns[turns.length - 1] = turns.last.copyWith(answer: m.content, streaming: false);
      }
    }
    state = RecallConversation(threadId: threadId, title: title, turns: turns);
  }

  /// Ask the next question — APPENDS a turn and CONTINUES the thread when one exists (REC-R7).
  void ask(String question) {
    final q = question.trim();
    if (q.isEmpty) return;
    final convo = state ?? const RecallConversation();
    if (convo.streaming) return; // single-flight within a turn
    _sub?.cancel();
    final next = convo.copyWith(turns: [...convo.turns, RecallTurn(question: q)]);
    state = next;
    // v0.2.2: apply the per-recall model/effort override (null = server default).
    final sel = ref.read(recallSelectionProvider);
    _sub = ref
        .read(apiClientProvider)
        .recallStream(
          q,
          threadId: convo.threadId,
          provider: sel.provider,
          model: sel.model,
          effort: sel.effort?.wire,
        )
        .listen(
          _onEvent,
          onError: (Object e) => _patchLast((t) => t.copyWith(streaming: false, error: e.toString())),
          onDone: () => _patchLast((t) => t.copyWith(streaming: false)),
        );
  }

  void _patchLast(RecallTurn Function(RecallTurn) f) {
    final convo = state;
    if (convo == null || convo.turns.isEmpty) return;
    state = convo.withLastTurn(f(convo.turns.last));
  }

  void _onEvent(SseEvent e) {
    switch (e) {
      case SessionStarted(:final threadId):
        // capture the server thread id so the NEXT ask continues this thread (REC-R7).
        final convo = state;
        if (convo != null && (convo.threadId == null || convo.threadId!.isEmpty)) {
          state = convo.copyWith(threadId: threadId);
        }
      case MessageDelta(:final text):
        _patchLast((t) => t.copyWith(answer: t.answer + text));
      case ReasoningDelta(:final text):
        _patchLast((t) => t.copyWith(reasoning: t.reasoning + text));
      case CitationEvent(:final citation):
        _patchLast((t) => t.copyWith(citations: [...t.citations, citation]));
      case DoneEvent():
        _patchLast((t) => t.copyWith(streaming: false));
        // A finished turn may have created a conversation and edited the wiki / curated
        // memory server-side; bump so the history drawer + wiki re-read (REC-R8, no relaunch).
        ref.read(cacheRevisionProvider.notifier).bump();
      case ErrorEvent(:final message):
        _patchLast((t) => t.copyWith(streaming: false, error: message));
      default:
        break;
    }
  }
}

final recallProvider =
    NotifierProvider<RecallNotifier, RecallConversation?>(RecallNotifier.new);
