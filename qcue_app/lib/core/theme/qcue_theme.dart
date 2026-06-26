// QCue S4-R12/R45: the QCueColors ThemeExtension carries the active semantic
// token map; widgets read it via `context.q` and never touch hex.
import 'package:flutter/material.dart';
import 'qcue_text.dart';
import 'qcue_tokens.dart';

/// ThemeExtension carrying the semantic token map. Read via `context.q`.
@immutable
class QCueColors extends ThemeExtension<QCueColors> {
  const QCueColors(this._map);
  final Map<QToken, Color> _map;

  factory QCueColors.of(QThemeId id) => QCueColors(qThemeColors(id));

  Color color(QToken t) => _map[t]!;

  // Convenience getters used pervasively by widgets.
  Color get bg => color(QToken.bg);
  Color get surface => color(QToken.surface);
  Color get surface2 => color(QToken.surface2);
  Color get border => color(QToken.border);
  Color get text => color(QToken.text);
  Color get text2 => color(QToken.text2);
  Color get text3 => color(QToken.text3);
  Color get accent => color(QToken.accent);
  Color get linkText => color(QToken.linkText);
  Color get success => color(QToken.success);
  Color get pending => color(QToken.pending);
  Color get info => color(QToken.info);
  Color get danger => color(QToken.danger);

  @override
  QCueColors copyWith() => QCueColors(_map);

  @override
  QCueColors lerp(ThemeExtension<QCueColors>? other, double t) {
    if (other is! QCueColors) return this;
    final lerped = <QToken, Color>{};
    for (final k in QToken.values) {
      lerped[k] = Color.lerp(_map[k], other._map[k], t)!;
    }
    return QCueColors(lerped);
  }
}

extension QCueColorsContext on BuildContext {
  QCueColors get q => Theme.of(this).extension<QCueColors>()!;
}

/// Builds a [ThemeData] from a semantic token map. The only `Theme`-level
/// hex comes from the token map (master §10); everything else is derived.
class QCueTheme {
  static ThemeData build(QThemeId id) {
    final colors = QCueColors.of(id);
    final isDark = id == QThemeId.night;
    final base = ThemeData(
      brightness: isDark ? Brightness.dark : Brightness.light,
      useMaterial3: true,
      scaffoldBackgroundColor: colors.bg,
      colorScheme: ColorScheme.fromSeed(
        seedColor: colors.accent,
        brightness: isDark ? Brightness.dark : Brightness.light,
      ).copyWith(
        surface: colors.bg,
        primary: colors.accent,
        error: colors.danger,
      ),
      extensions: <ThemeExtension<dynamic>>[colors],
    );
    return base.copyWith(textTheme: QCueText.textThemeFor(base.textTheme, colors.text));
  }
}
