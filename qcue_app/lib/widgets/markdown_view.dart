// QCue S4-R57/R63 (v0.2.2): the wiki body renderer. It now delegates to
// [QMarkdown] — the themed, INERT GFM renderer (gpt_markdown) — so a wiki body
// gets full markdown (headings, bold/italic/strike, inline + fenced code,
// tables, ordered/unordered/nested lists, blockquotes, task lists) with QCue's
// `[[wikilinks]]` preserved and routed. Kept as a named widget so existing call
// sites (wiki_page_screen) need no change. Inert: links route, nothing executes.
import 'package:flutter/material.dart';
import 'q_markdown.dart';

class MarkdownView extends StatelessWidget {
  const MarkdownView(this.source, {super.key, this.onTapLink});

  final String source;
  final void Function(String slug)? onTapLink;

  @override
  Widget build(BuildContext context) => QMarkdown(source, onTapLink: onTapLink);
}
