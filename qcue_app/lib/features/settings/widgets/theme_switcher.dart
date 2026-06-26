// QCue S4-R45: the 3-theme switcher (Clean Light / Anthropic Warm / Night).
// Live + persisted via the single themeProvider source of truth. Each row is a
// ≥44pt target with an accessible, selection-aware label.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../../core/theme/qcue_text.dart';
import '../../../core/theme/qcue_theme.dart';
import '../../../core/theme/qcue_tokens.dart';
import '../../../core/theme/theme_provider.dart';

const _labels = {
  QThemeId.cleanLight: 'Clean Light',
  QThemeId.anthropicWarm: 'Anthropic Warm',
  QThemeId.night: 'Night',
};

class ThemeSwitcher extends ConsumerWidget {
  const ThemeSwitcher({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final active = ref.watch(themeProvider);
    return Column(
      children: [
        for (final id in QThemeId.values)
          Semantics(
            selected: id == active,
            button: true,
            label: '${_labels[id]} theme${id == active ? ', selected' : ''}',
            child: ListTile(
              minVerticalPadding: 12,
              leading: _Swatch(id: id),
              title: Text(_labels[id]!,
                  style: QCueText.body.copyWith(color: context.q.text)),
              trailing:
                  id == active ? Icon(Icons.check, color: context.q.accent) : null,
              onTap: () => ref.read(themeProvider.notifier).select(id),
            ),
          ),
      ],
    );
  }
}

/// A tiny preview of a theme's bg + accent (mirrors the existing settings swatch).
class _Swatch extends StatelessWidget {
  const _Swatch({required this.id});
  final QThemeId id;

  @override
  Widget build(BuildContext context) {
    final swatch = qThemeColors(id);
    return Container(
      width: 28,
      height: 28,
      decoration: BoxDecoration(
        color: swatch[QToken.bg],
        border: Border.all(color: context.q.border),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Center(
        child: Container(
          width: 12,
          height: 12,
          decoration: BoxDecoration(
            color: swatch[QToken.accent],
            shape: BoxShape.circle,
          ),
        ),
      ),
    );
  }
}
