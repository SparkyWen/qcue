// QCue S4-R37/R38/R39: the Recall chat — a clean conversation. Input pinned at
// the bottom; on submit, the answer streams in token-by-token (StreamingText
// over message_delta), with tappable inline [[links]] (WikiLinkText via
// StreamingText), citation chips (CitationChip from citation events), and a
// collapsed-by-default reasoning disclosure (D18). Generous spacing,
// content-first; empty state before the first question.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import '../../widgets/citation_chip.dart';
import '../../widgets/empty_state.dart';
import '../../widgets/reasoning_disclosure.dart';
import '../../widgets/streaming_text.dart';
import 'history_drawer.dart';
import 'recall_provider.dart';
import 'widgets/intelligence_selector.dart';

class RecallScreen extends ConsumerWidget {
  const RecallScreen({super.key, this.onOpenPage, this.onOpenCitation});

  final void Function(String slug)? onOpenPage;
  final void Function(String ref)? onOpenCitation;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final convo = ref.watch(recallProvider);
    // REC-D8: the app's shared AppScaffold has no Drawer, so the recall history
    // drawer lives on a NESTED Scaffold local to this screen.
    return Scaffold(
      // The page background token (flat themes ⇒ visually identical to the host
      // AppScaffold); a raw transparent constant would trip the no-raw-hex arch test.
      backgroundColor: context.q.bg,
      drawer: const HistoryDrawer(),
      body: Column(
        children: [
          // A slim header with the history (drawer) button.
          Builder(
            builder: (ctx) => Align(
              alignment: Alignment.centerLeft,
              child: IconButton(
                key: const ValueKey('recall-history-button'),
                tooltip: 'Conversation history',
                icon: Icon(Icons.history, color: context.q.text2),
                onPressed: () => Scaffold.of(ctx).openDrawer(),
              ),
            ),
          ),
          Expanded(
            child: (convo == null || convo.turns.isEmpty)
                ? const EmptyState(
                    key: ValueKey('recall-empty'),
                    icon: Icons.auto_awesome_outlined,
                    title: 'Ask your second brain anything',
                    hint: 'Recall cites the exact source line.',
                  )
                : _Conversation(
                    convo: convo,
                    onOpenPage: onOpenPage,
                    onOpenCitation: onOpenCitation,
                  ),
          ),
          Divider(height: 1, color: context.q.border),
          _RecallInput(
            // S4-R37: the composer is single-flight — disabled while a turn streams.
            streaming: convo?.streaming ?? false,
            onSubmit: (q) => ref.read(recallProvider.notifier).ask(q),
          ),
        ],
      ),
    );
  }
}

class _Conversation extends StatelessWidget {
  const _Conversation({
    required this.convo,
    required this.onOpenPage,
    required this.onOpenCitation,
  });

  final RecallConversation convo;
  final void Function(String slug)? onOpenPage;
  final void Function(String ref)? onOpenCitation;

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(QSpace.md),
      children: [
        for (final turn in convo.turns) ...[
          // The question, quietly above the answer.
          Text(turn.question,
              style: QCueText.label.copyWith(color: context.q.text2)),
          const SizedBox(height: QSpace.md),
          // Reasoning first (collapsed), then the streamed answer.
          ReasoningDisclosure(reasoning: turn.reasoning),
          if (turn.answer.isNotEmpty) ...[
            const SizedBox(height: QSpace.xs),
            StreamingText(
              text: turn.answer,
              streaming: turn.streaming,
              onTapLink: onOpenPage,
            ),
          ] else if (turn.streaming)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: QSpace.sm),
              child: Text('Thinking…',
                  style: QCueText.caption.copyWith(color: context.q.text3)),
            ),
          if (turn.error != null) ...[
            const SizedBox(height: QSpace.sm),
            Text("Couldn't answer · ${turn.error}",
                style: QCueText.caption.copyWith(color: context.q.danger)),
          ],
          if (turn.citations.isNotEmpty) ...[
            const SizedBox(height: QSpace.md),
            Wrap(
              spacing: QSpace.sm,
              runSpacing: QSpace.sm,
              children: [
                for (final c in turn.citations)
                  CitationChip(
                    citation: c,
                    onTap: (cit) =>
                        onOpenCitation?.call('${cit.relPath}:${cit.startLine}'),
                  ),
              ],
            ),
          ],
          const SizedBox(height: QSpace.lg),
        ],
      ],
    );
  }
}

class _RecallInput extends ConsumerStatefulWidget {
  const _RecallInput({required this.onSubmit, this.streaming = false});
  final void Function(String question) onSubmit;

  /// S4-R37: true while a turn streams — the composer disables its send so a
  /// second ask cannot interrupt the in-flight answer (single-flight).
  final bool streaming;
  @override
  ConsumerState<_RecallInput> createState() => _RecallInputState();
}

class _RecallInputState extends ConsumerState<_RecallInput> {
  final _controller = TextEditingController();

  void _submit() {
    if (widget.streaming) return; // single-flight: ignore while a turn streams
    final q = _controller.text.trim();
    if (q.isEmpty) return;
    widget.onSubmit(q);
    _controller.clear();
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(QSpace.md),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          Expanded(
            child: Semantics(
              textField: true,
              label: 'Ask a question',
              child: TextField(
                key: const ValueKey('recall-input'),
                controller: _controller,
                minLines: 1,
                maxLines: 4,
                textInputAction: TextInputAction.send,
                onSubmitted: (_) => _submit(),
                style: QCueText.body.copyWith(color: context.q.text),
                decoration: InputDecoration(
                  hintText: 'Ask anything…',
                  hintStyle: QCueText.body.copyWith(color: context.q.text3),
                  filled: true,
                  fillColor: context.q.surface,
                  border: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(QRadius.control),
                    borderSide: BorderSide(color: context.q.border),
                  ),
                  enabledBorder: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(QRadius.control),
                    borderSide: BorderSide(color: context.q.border),
                  ),
                ),
              ),
            ),
          ),
          const SizedBox(width: QSpace.sm),
          // Pick the Intelligence effort + model for this recall (left of send).
          // Disabled while a turn streams, mirroring the send button.
          IntelligenceSelector(enabled: !widget.streaming),
          const SizedBox(width: QSpace.sm),
          ConstrainedBox(
            constraints: const BoxConstraints(minWidth: 44, minHeight: 44),
            child: IconButton(
              key: const ValueKey('recall-send'),
              tooltip: widget.streaming ? 'Answering…' : 'Ask',
              // S4-R37: disabled while streaming (null onPressed greys it out).
              onPressed: widget.streaming ? null : _submit,
              // S4-R37: greyed while streaming; the null onPressed also disables it.
              // (A static icon, not a spinner — an indeterminate spinner would
              // never let widget tests settle. The "Thinking…" text shows progress.)
              icon: Icon(
                Icons.arrow_upward,
                color: widget.streaming ? context.q.text3 : context.q.accent,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
