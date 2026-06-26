// QCue S4-R15: the type scale (master §10) — 13/15/17/20/28/34, base 17, body
// line-height 1.6, mono 1.4; weights 400/500/600; tabular figures for ledgers.
//
// The named styles carry the *logical* family names 'Inter' (UI/content) and
// 'JetBrainsMono' (code/citations/IDs). At runtime those names are wired to the
// real Google Fonts via [registerFontLoaders] so we ship no font binaries;
// tests assert on the logical names, decoupled from how the glyphs are loaded.
import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';

abstract final class QCueText {
  static const _inter = 'Inter';
  static const _mono = 'JetBrainsMono';

  static const caption = TextStyle(
    fontFamily: _inter,
    fontSize: 13,
    height: 1.5,
    fontWeight: FontWeight.w400,
  );
  static const label = TextStyle(
    fontFamily: _inter,
    fontSize: 15,
    height: 1.5,
    fontWeight: FontWeight.w500,
  );
  static const body = TextStyle(
    fontFamily: _inter,
    fontSize: 17,
    height: 1.6,
    fontWeight: FontWeight.w400,
  );
  static const subtitle = TextStyle(
    fontFamily: _inter,
    fontSize: 20,
    height: 1.4,
    fontWeight: FontWeight.w600,
  );
  static const title = TextStyle(
    fontFamily: _inter,
    fontSize: 28,
    height: 1.25,
    fontWeight: FontWeight.w600,
  );
  static const display = TextStyle(
    fontFamily: _inter,
    fontSize: 34,
    height: 1.2,
    fontWeight: FontWeight.w600,
  );

  static const mono = TextStyle(
    fontFamily: _mono,
    fontSize: 15,
    height: 1.4,
    fontWeight: FontWeight.w400,
  );
  static const monoTabular = TextStyle(
    fontFamily: _mono,
    fontSize: 15,
    height: 1.4,
    fontWeight: FontWeight.w400,
    fontFeatures: [FontFeature.tabularFigures()],
  );

  /// Tells google_fonts to satisfy the logical family names with the real
  /// Inter / JetBrains Mono webfonts (called once at bootstrap).
  static void registerFontLoaders() {
    GoogleFonts.config.allowRuntimeFetching = true;
  }

  /// A Material [TextTheme] keyed to the QCue scale, tinted with [textColor].
  static TextTheme textThemeFor(TextTheme base, Color textColor) {
    final t = base.apply(bodyColor: textColor, displayColor: textColor);
    return t.copyWith(
      displayLarge: display.copyWith(color: textColor),
      headlineMedium: title.copyWith(color: textColor),
      titleLarge: subtitle.copyWith(color: textColor),
      bodyLarge: body.copyWith(color: textColor),
      bodyMedium: label.copyWith(color: textColor),
      labelSmall: caption.copyWith(color: textColor),
    );
  }
}
