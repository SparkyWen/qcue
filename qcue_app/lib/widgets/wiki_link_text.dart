// QCue S4-R34/R57: inline `[[wikilink]]` text. Renders a paragraph that may
// embed `[[Display]]` or `[[slug|Display]]` links. Plain runs are body text on
// the `text` token; each link is a tappable `linkText`-colored span that routes
// to its slug. The slug is taken explicitly (`slug|Display`) or slugified from
// the display. This is the single inline-link parser reused by the markdown
// body renderer and the recall answer.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import '../core/theme/qcue_text.dart';
import '../core/theme/qcue_theme.dart';

/// Slugify a display name into a wiki slug (lowercase, words joined by `-`).
String slugifyWikiTarget(String raw) {
  final t = raw.trim().toLowerCase();
  final cleaned = t.replaceAll(RegExp(r'[^a-z0-9]+'), '-');
  return cleaned.replaceAll(RegExp(r'^-+|-+$'), '');
}

/// One parsed segment of a paragraph: literal text or a wiki link.
sealed class WikiSegment {
  const WikiSegment();
}

class TextSegment extends WikiSegment {
  const TextSegment(this.text);
  final String text;
}

class LinkSegment extends WikiSegment {
  const LinkSegment({required this.slug, required this.display});
  final String slug;
  final String display;
}

final _wikiLinkRe = RegExp(r'\[\[([^\]]+)\]\]');

/// Splits [source] into literal-text and `[[wikilink]]` segments.
List<WikiSegment> parseWikiSegments(String source) {
  final out = <WikiSegment>[];
  var last = 0;
  for (final m in _wikiLinkRe.allMatches(source)) {
    if (m.start > last) out.add(TextSegment(source.substring(last, m.start)));
    final inner = m.group(1)!;
    final pipe = inner.indexOf('|');
    final String slug;
    final String display;
    if (pipe >= 0) {
      slug = slugifyWikiTarget(inner.substring(0, pipe));
      display = inner.substring(pipe + 1).trim();
    } else {
      display = inner.trim();
      slug = slugifyWikiTarget(display);
    }
    out.add(LinkSegment(slug: slug, display: display));
    last = m.end;
  }
  if (last < source.length) out.add(TextSegment(source.substring(last)));
  return out;
}

class WikiLinkText extends StatefulWidget {
  const WikiLinkText(
    this.source, {
    super.key,
    this.onTapLink,
    this.style,
  });

  final String source;
  final void Function(String slug)? onTapLink;
  final TextStyle? style;

  @override
  State<WikiLinkText> createState() => _WikiLinkTextState();
}

class _WikiLinkTextState extends State<WikiLinkText> {
  final _recognizers = <TapGestureRecognizer>[];

  @override
  void dispose() {
    for (final r in _recognizers) {
      r.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    for (final r in _recognizers) {
      r.dispose();
    }
    _recognizers.clear();

    final base = (widget.style ?? QCueText.body).copyWith(color: context.q.text);
    final linkStyle = base.copyWith(color: context.q.linkText);
    final segments = parseWikiSegments(widget.source);

    final spans = <InlineSpan>[];
    for (final seg in segments) {
      switch (seg) {
        case TextSegment(:final text):
          spans.add(TextSpan(text: text, style: base));
        case LinkSegment(:final slug, :final display):
          final recognizer = TapGestureRecognizer()
            ..onTap = () => widget.onTapLink?.call(slug);
          _recognizers.add(recognizer);
          spans.add(TextSpan(
            text: display,
            style: linkStyle,
            recognizer: recognizer,
            semanticsLabel: 'link, $display',
          ));
      }
    }
    return Text.rich(TextSpan(children: spans));
  }
}
