// QCue S4-R42 / A-R37: pure Dream state + reducer. Folds the dream SSE taxonomy
// (dream_started → progress* → completed|failed) into a growing DreamState. The
// progress watcher collapses tool-uses to a per-turn count and dedups touched
// pages ("at least these were touched", A-R15). Reasoning accumulates separately
// and is rendered collapsed-by-default (D18).
import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/sse_event.dart';
import '../../core/net/api_client_provider.dart';

enum DreamStatus { running, completed, failed }

class DreamTurn {
  const DreamTurn({
    required this.text,
    required this.toolUseCount,
    required this.pagesTouched,
  });
  final String text;
  final int toolUseCount;
  final List<String> pagesTouched;
}

class DreamState {
  const DreamState({
    required this.title,
    required this.status,
    this.sessionsReviewing = 0,
    this.pagesTouched = const [],
    this.turns = const [],
    this.elapsed = Duration.zero,
    this.reasoning = '',
    this.pagesImproved,
    this.error,
  });

  final String title;
  final DreamStatus status;
  final int sessionsReviewing;
  final List<String> pagesTouched;
  final List<DreamTurn> turns;
  final Duration elapsed;
  final String reasoning;
  final int? pagesImproved;
  final String? error;

  DreamState copyWith({
    DreamStatus? status,
    int? sessionsReviewing,
    List<String>? pagesTouched,
    List<DreamTurn>? turns,
    Duration? elapsed,
    String? reasoning,
    int? pagesImproved,
    String? error,
  }) =>
      DreamState(
        title: title,
        status: status ?? this.status,
        sessionsReviewing: sessionsReviewing ?? this.sessionsReviewing,
        pagesTouched: pagesTouched ?? this.pagesTouched,
        turns: turns ?? this.turns,
        elapsed: elapsed ?? this.elapsed,
        reasoning: reasoning ?? this.reasoning,
        pagesImproved: pagesImproved ?? this.pagesImproved,
        error: error ?? this.error,
      );
}

/// Pure reducer folding a dream SSE event into state (S4-R42). Tool-uses collapse
/// to a count; touched pages dedup; reasoning accumulates collapsed (D18).
DreamState applyDreamEvent(DreamState s, SseEvent e) => switch (e) {
      DreamStarted(:final sessions) => s.copyWith(sessionsReviewing: sessions),
      DreamProgress(:final text, :final toolUseCount, :final pagesTouched) =>
        s.copyWith(
          turns: [
            ...s.turns,
            DreamTurn(
                text: text,
                toolUseCount: toolUseCount,
                pagesTouched: pagesTouched),
          ],
          pagesTouched: {...s.pagesTouched, ...pagesTouched}.toList(),
        ),
      ReasoningDelta(:final text) => s.copyWith(reasoning: s.reasoning + text),
      DreamCompleted(:final pagesImproved) => s.copyWith(
          status: DreamStatus.completed, pagesImproved: pagesImproved),
      DreamFailed(:final reason) =>
        s.copyWith(status: DreamStatus.failed, error: reason),
      _ => s,
    };

/// Live Dream state for a running job: subscribes to the SSE dream stream, folds
/// each event, and ticks a 1s elapsed clock while running (A-R37 live elapsed).
class DreamNotifier extends FamilyNotifier<DreamState, String> {
  StreamSubscription<SseEvent>? _sub;
  Timer? _ticker;
  DateTime? _startedAt;

  @override
  DreamState build(String jobId) {
    _startedAt = DateTime.now();
    final api = ref.read(apiClientProvider);
    _sub = api.dreamEvents(jobId).listen(
          (e) => state = applyDreamEvent(state, e),
          onDone: () => _ticker?.cancel(),
        );
    _ticker = Timer.periodic(const Duration(seconds: 1), (_) {
      if (state.status != DreamStatus.running) {
        _ticker?.cancel();
        return;
      }
      state = state.copyWith(
          elapsed: DateTime.now().difference(_startedAt!));
    });
    ref.onDispose(() {
      _sub?.cancel();
      _ticker?.cancel();
    });
    return const DreamState(
        title: 'Memory consolidation', status: DreamStatus.running);
  }
}

final dreamProvider =
    NotifierProvider.family<DreamNotifier, DreamState, String>(
        DreamNotifier.new);
