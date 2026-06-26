// QCue v0.2.2: a themed, INERT markdown renderer used for wiki bodies and recall
// answers. Wraps `gpt_markdown` (GFM: headings, bold/italic/strike, inline +
// fenced code, links, tables, ordered/unordered/nested lists, blockquotes, task
// lists) and styles it ENTIRELY through `context.q` design tokens — no raw hex,
// so it passes the no-raw-hex arch test and re-themes live across all 3 themes.
//
// It preserves QCue's custom `[[wikilink]]` syntax by rewriting links to a
// `wiki:<slug>` href that `onLinkTap` intercepts and routes back to
// `onTapLink(slug)` (the same slugify rule as WikiLinkText). It is INERT
// (S4-R57 spirit): code blocks render but never execute, and non-wiki links are
// NOT auto-launched — an untrusted body can neither run code nor open the web.
import 'package:flutter/material.dart';
import 'package:gpt_markdown/gpt_markdown.dart';
import '../core/theme/qcue_space.dart';
import '../core/theme/qcue_text.dart';
import '../core/theme/qcue_theme.dart';
import 'wiki_link_text.dart' show slugifyWikiTarget;

final _wikiLink = RegExp(r'\[\[([^\]]+)\]\]');

/// Rewrites `[[Display]]` / `[[slug|Display]]` into a `[Display](wiki:slug)`
/// markdown link so a single markdown pass renders them. The `wiki:` scheme is
/// intercepted by [QMarkdown]'s onLinkTap and routed to `onTapLink(slug)`.
String preprocessWikiLinks(String src) =>
    src.replaceAllMapped(_wikiLink, (m) {
      final inner = m.group(1)!;
      final pipe = inner.indexOf('|');
      final target = pipe >= 0 ? inner.substring(0, pipe) : inner;
      final display = (pipe >= 0 ? inner.substring(pipe + 1) : inner).trim();
      return '[$display](wiki:${slugifyWikiTarget(target)})';
    });

/// The wiki slug carried by a `wiki:<slug>` link href, or null for any other
/// (external) link. The single place the wiki: scheme is decoded.
String? wikiSlugFromUrl(String url) =>
    url.startsWith('wiki:') ? url.substring('wiki:'.length) : null;

class QMarkdown extends StatelessWidget {
  const QMarkdown(this.source, {super.key, this.onTapLink, this.style});

  final String source;

  /// Tapping a `[[wikilink]]` routes here with its slug.
  final void Function(String slug)? onTapLink;

  /// Base body text style (color is forced to `context.q.text`).
  final TextStyle? style;

  @override
  Widget build(BuildContext context) {
    final q = context.q;
    final base = (style ?? QCueText.body).copyWith(color: q.text);
    final brightness = Theme.of(context).brightness;

    return GptMarkdownTheme(
      gptThemeData: GptMarkdownThemeData(
        brightness: brightness,
        // Links/headers/rules all on tokens (no raw hex → arch test passes).
        linkColor: q.linkText,
        linkHoverColor: q.linkText,
        highlightColor: q.surface2,
        hrLineColor: q.border,
        h1: QCueText.title.copyWith(color: q.text),
        h2: QCueText.subtitle.copyWith(color: q.text),
        h3: QCueText.label.copyWith(color: q.text, fontWeight: FontWeight.w700),
        h4: QCueText.label.copyWith(color: q.text, fontWeight: FontWeight.w600),
        h5: QCueText.body.copyWith(color: q.text, fontWeight: FontWeight.w600),
        h6: QCueText.caption
            .copyWith(color: q.text2, fontWeight: FontWeight.w600),
        // QCue headings are self-spaced; no auto divider after an h1.
        autoAddDividerLineAfterH1: false,
      ),
      child: GptMarkdown(
        preprocessWikiLinks(source),
        style: base,
        onLinkTap: (url, title) {
          final slug = wikiSlugFromUrl(url);
          if (slug != null) onTapLink?.call(slug);
          // Non-wiki links are inert — never auto-launched in-app.
        },
        codeBuilder: (ctx, name, code, closed) => _CodeBlock(code: code),
        highlightBuilder: (ctx, text, _) => _InlineCode(text: text),
      ),
    );
  }
}

/// A fenced ``` code block — monospace on a surface, inert (never executed).
/// Keeps the `md-code-block` key the wiki body tests assert on.
class _CodeBlock extends StatelessWidget {
  const _CodeBlock({required this.code});
  final String code;
  @override
  Widget build(BuildContext context) {
    final q = context.q;
    return Container(
      width: double.infinity,
      margin: const EdgeInsets.symmetric(vertical: QSpace.xs),
      padding: const EdgeInsets.all(QSpace.sm),
      decoration: BoxDecoration(
        color: q.surface2,
        borderRadius: BorderRadius.circular(QRadius.input),
      ),
      child: Text(
        code.trimRight(),
        key: const ValueKey('md-code-block'),
        style: QCueText.mono.copyWith(color: q.text2),
      ),
    );
  }
}

/// Inline `code` — a small monospace chip on the surface tint.
class _InlineCode extends StatelessWidget {
  const _InlineCode({required this.text});
  final String text;
  @override
  Widget build(BuildContext context) {
    final q = context.q;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
      decoration: BoxDecoration(
        color: q.surface2,
        borderRadius: BorderRadius.circular(4),
      ),
      child: Text(text,
          style: QCueText.mono.copyWith(color: q.text2, fontSize: 14)),
    );
  }
}
