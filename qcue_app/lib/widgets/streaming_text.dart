// QCue S4-R37 (v0.2.2): the streamed recall answer. Renders the accumulated
// text token-by-token (the caller feeds it grown text from message_delta
// events) as full markdown via [QMarkdown] — headings, bold/italic, inline +
// fenced code, tables, lists, blockquotes — with `[[wikilinks]]` preserved and
// routed. While streaming, a subtle caret trails the text; it disappears on
// completion. QMarkdown re-parses the grown string each tick (cheap for an
// answer-sized body) and renders partial/unterminated markup gracefully.
import 'package:flutter/material.dart';
import '../core/theme/qcue_theme.dart';
import 'q_markdown.dart';

class StreamingText extends StatelessWidget {
  const StreamingText({
    super.key,
    required this.text,
    required this.streaming,
    this.onTapLink,
  });

  final String text;
  final bool streaming;
  final void Function(String slug)? onTapLink;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        QMarkdown(text, onTapLink: onTapLink),
        if (streaming)
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: Container(
              key: const ValueKey('stream-caret'),
              width: 2,
              height: 18,
              color: context.q.text2,
            ),
          ),
      ],
    );
  }
}
