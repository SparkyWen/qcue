// QCue S4-R12: the ONLY file permitted to contain raw color literals.
// Hex values are verbatim from master §10 (the authoritative palette).
import 'dart:ui';

/// Semantic color roles (master §10). Widgets reference these, never hex.
enum QToken {
  bg,
  surface,
  surface2,
  border,
  text,
  text2,
  text3,
  accent,
  // Reading-weight color for inline [[wikilink]] TEXT (Inter body). Distinct
  // from `accent` (CTA *fills*, white label): linkText must itself clear 4.5:1
  // normal-text AA against the page bg/surface in every theme.
  linkText,
  success,
  pending,
  info,
  danger,
}

/// The three shipped themes; Clean Light is the default (S4-R45).
enum QThemeId { cleanLight, anthropicWarm, night }

const Map<QToken, Color> _cleanLight = {
  QToken.bg: Color(0xFFFFFFFF),
  QToken.surface: Color(0xFFF7F7F5),
  QToken.surface2: Color(0xFFEFEFEE),
  QToken.border: Color(0xFFE9E9E7),
  QToken.text: Color(0xFF37352F),
  QToken.text2: Color(0xFF787774),
  QToken.text3: Color(0xFF9B9A97),
  QToken.accent: Color(0xFF2563EB),
  // Readable blue, 6.18:1 on bg / 5.76:1 on surface.
  QToken.linkText: Color(0xFF1A56DB),
  QToken.success: Color(0xFF16A34A),
  QToken.pending: Color(0xFFD97706),
  QToken.info: Color(0xFF2563EB),
  QToken.danger: Color(0xFFDC2626),
};

const Map<QToken, Color> _anthropicWarm = {
  QToken.bg: Color(0xFFFAF9F5),
  QToken.surface: Color(0xFFFFFFFF),
  QToken.surface2: Color(0xFFF3F0E8),
  QToken.border: Color(0xFFEDEAE0),
  QToken.text: Color(0xFF1F1E1D),
  QToken.text2: Color(0xFF6B675F),
  QToken.text3: Color(0xFF928C7E),
  QToken.accent: Color(0xFFD97757),
  // Darker terracotta, 6.94:1 on bg / 6.81:1 on surface (clears 4.5; the clay
  // accent #D97757 is only 2.96 on cream and stays a CTA fill, not link text).
  QToken.linkText: Color(0xFF9A3412),
  QToken.success: Color(0xFF5E8C5A),
  QToken.pending: Color(0xFFC2410C),
  QToken.info: Color(0xFFB25B3A),
  QToken.danger: Color(0xFFB91C1C),
};

const Map<QToken, Color> _night = {
  QToken.bg: Color(0xFF191919),
  QToken.surface: Color(0xFF262626),
  QToken.surface2: Color(0xFF2F2F2F),
  QToken.border: Color(0xFF363432),
  QToken.text: Color(0xFFE6E6E5),
  QToken.text2: Color(0xFF9B9A97),
  QToken.text3: Color(0xFF6F6E6B),
  QToken.accent: Color(0xFF5B8DEF),
  // Same blue as accent, 5.44:1 on bg / 4.69:1 on surface (clears 4.5).
  QToken.linkText: Color(0xFF5B8DEF),
  QToken.success: Color(0xFF4ADE80),
  QToken.pending: Color(0xFFFBBF24),
  QToken.info: Color(0xFF7AA2F7),
  QToken.danger: Color(0xFFF87171),
};

/// Resolve a theme's full token map.
Map<QToken, Color> qThemeColors(QThemeId id) => switch (id) {
      QThemeId.cleanLight => _cleanLight,
      QThemeId.anthropicWarm => _anthropicWarm,
      QThemeId.night => _night,
    };
