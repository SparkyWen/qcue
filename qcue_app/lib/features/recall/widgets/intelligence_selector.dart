// QCue: the recall composer's "Intelligence + Model" selector — a ChatGPT-mobile-style
// two-level chooser. Level 1 is a lightweight popover: a list of reasoning-effort levels
// (Instant/Medium/High/Extra High/Pro) plus a model entry row. Tapping the model row opens
// Level 2, a bottom sheet listing the configured BYOK models grouped by provider. Reads the
// vault via settingsProvider and writes to recallSelectionProvider (forwarded to the backend
// by RecallNotifier.ask). Styled entirely via context.q tokens (no raw hex).
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../../core/theme/qcue_space.dart';
import '../../../core/theme/qcue_text.dart';
import '../../../core/theme/qcue_theme.dart';
import '../../settings/settings_provider.dart';
import '../model_display.dart';
import '../recall_selection.dart';

/// The composer entry pill, e.g. "GPT-5.5 · Instant". Tapping opens Level 1.
class IntelligenceSelector extends ConsumerWidget {
  const IntelligenceSelector({super.key, required this.enabled});

  /// Mirrors the send button: disabled while a turn streams.
  final bool enabled;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final sel = ref.watch(recallSelectionProvider);
    final q = context.q;
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 180, minHeight: 44),
      child: Material(
        type: MaterialType.transparency,
        child: InkWell(
          key: const ValueKey('intelligence-selector'),
          borderRadius: BorderRadius.circular(QRadius.control),
          onTap: enabled ? () => _openLevel1(context) : null,
          child: Container(
            padding: const EdgeInsets.symmetric(
                horizontal: QSpace.sm, vertical: QSpace.xs),
            decoration: BoxDecoration(
              color: q.surface,
              borderRadius: BorderRadius.circular(QRadius.control),
              border: Border.all(color: q.border),
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Flexible(
                  child: Text(
                    selectorPillLabel(sel),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: QCueText.caption
                        .copyWith(color: enabled ? q.text2 : q.text3),
                  ),
                ),
                const SizedBox(width: QSpace.xs),
                Icon(Icons.expand_more,
                    size: 16, color: enabled ? q.text2 : q.text3),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Level 1 — the effort popover. The [composerContext] is captured so Level 2 can be opened
/// after this popover closes.
Future<void> _openLevel1(BuildContext composerContext) {
  return showDialog<void>(
    context: composerContext,
    builder: (_) => _IntelligencePopover(composerContext: composerContext),
  );
}

/// Level 2 — the model bottom sheet. Public so it can be opened directly if needed.
Future<void> showModelSheet(BuildContext context) {
  return showModalBottomSheet<void>(
    context: context,
    isScrollControlled: true,
    backgroundColor: context.q.bg,
    builder: (_) => const _ModelSheet(),
  );
}

class _IntelligencePopover extends ConsumerWidget {
  const _IntelligencePopover({required this.composerContext});
  final BuildContext composerContext;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final sel = ref.watch(recallSelectionProvider);
    final q = context.q;
    return Align(
      alignment: const Alignment(0, -0.3),
      child: Padding(
        padding: const EdgeInsets.all(QSpace.lg),
        child: Material(
          color: q.bg,
          elevation: 8,
          borderRadius: BorderRadius.circular(QRadius.card),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 320),
            child: Column(
              key: const ValueKey('intelligence-sheet'),
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(
                      QSpace.md, QSpace.md, QSpace.md, QSpace.xs),
                  child: Semantics(
                    header: true,
                    child: Text('Intelligence',
                        style: QCueText.label.copyWith(color: q.text3)),
                  ),
                ),
                for (final lvl in intelligenceLevels)
                  _OptionRow(
                    key: ValueKey('effort-opt-${lvl.effort.wire}'),
                    label: lvl.label,
                    selected: sel.effort == lvl.effort,
                    onTap: () {
                      ref
                          .read(recallSelectionProvider.notifier)
                          .setEffort(lvl.effort);
                      Navigator.of(context).pop();
                    },
                  ),
                Divider(height: 1, color: q.border),
                ListTile(
                  key: const ValueKey('intelligence-model-entry'),
                  dense: true,
                  title: Text(
                    sel.model != null ? modelDisplayName(sel.model!) : 'Auto',
                    style: QCueText.body.copyWith(color: q.text),
                  ),
                  trailing:
                      Icon(Icons.chevron_right, color: q.text2, size: 20),
                  onTap: () {
                    Navigator.of(context).pop();
                    showModelSheet(composerContext);
                  },
                ),
                const SizedBox(height: QSpace.sm),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _ModelSheet extends ConsumerWidget {
  const _ModelSheet();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(settingsProvider);
    final sel = ref.watch(recallSelectionProvider);
    final q = context.q;
    return SafeArea(
      child: SingleChildScrollView(
        key: const ValueKey('model-sheet'),
        padding: const EdgeInsets.only(bottom: QSpace.lg),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                  QSpace.md, QSpace.md, QSpace.md, QSpace.xs),
              child: Row(
                children: [
                  Text(
                    sel.model != null ? modelDisplayName(sel.model!) : 'Auto',
                    style: QCueText.subtitle.copyWith(color: q.text),
                  ),
                  const SizedBox(width: QSpace.xs),
                  Icon(Icons.expand_more, color: q.text2, size: 18),
                ],
              ),
            ),
            Divider(height: 1, color: q.border),
            _ModelRow(
              id: 'default',
              label: 'Auto (server picks)',
              selected: sel.model == null,
              onTap: () {
                ref.read(recallSelectionProvider.notifier).clearModel();
                Navigator.of(context).pop();
              },
            ),
            ...switch (settings) {
              AsyncData(:final value) => _modelRows(context, ref, value, sel),
              AsyncLoading() => const [_Note('Loading your models…')],
              AsyncError() => const [_Note("Couldn't load your models.")],
              _ => const <Widget>[],
            },
          ],
        ),
      ),
    );
  }

  List<Widget> _modelRows(
      BuildContext context, WidgetRef ref, SettingsSnapshot s, RecallSelection sel) {
    if (s.credentials.isEmpty) {
      return const [_Note('Add an API key in Settings to pick a model.')];
    }
    final rows = <Widget>[];
    for (final c in s.credentials) {
      final models = s.models[c.provider] ?? const <String>[];
      if (models.isEmpty) continue;
      rows.add(_ProviderLabel(c.provider));
      for (final m in models) {
        rows.add(_ModelRow(
          id: m,
          label: modelDisplayName(m),
          selected: sel.provider == c.provider && sel.model == m,
          onTap: () {
            ref.read(recallSelectionProvider.notifier).setModel(c.provider, m);
            Navigator.of(context).pop();
          },
        ));
      }
    }
    if (rows.isEmpty) {
      return const [_Note('No models available for your keys yet.')];
    }
    return rows;
  }
}

class _OptionRow extends StatelessWidget {
  const _OptionRow({
    super.key,
    required this.label,
    required this.selected,
    required this.onTap,
  });
  final String label;
  final bool selected;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final q = context.q;
    return ListTile(
      dense: true,
      title: Text(label,
          style: QCueText.body
              .copyWith(color: selected ? q.accent : q.text)),
      trailing:
          selected ? Icon(Icons.check, color: q.accent, size: 20) : null,
      onTap: onTap,
    );
  }
}

class _ModelRow extends StatelessWidget {
  const _ModelRow({
    required this.id,
    required this.label,
    required this.selected,
    required this.onTap,
  });
  final String id;
  final String label;
  final bool selected;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final q = context.q;
    return ListTile(
      key: ValueKey('model-row-$id'),
      dense: true,
      title: Text(label,
          style: QCueText.body
              .copyWith(color: selected ? q.accent : q.text)),
      trailing:
          selected ? Icon(Icons.check, color: q.accent, size: 20) : null,
      onTap: onTap,
    );
  }
}

class _ProviderLabel extends StatelessWidget {
  const _ProviderLabel(this.provider);
  final String provider;
  @override
  Widget build(BuildContext context) => Padding(
        padding: const EdgeInsets.fromLTRB(
            QSpace.md, QSpace.sm, QSpace.md, QSpace.xs),
        child: Text(provider,
            style: QCueText.caption.copyWith(color: context.q.text3)),
      );
}

class _Note extends StatelessWidget {
  const _Note(this.text);
  final String text;
  @override
  Widget build(BuildContext context) => Padding(
        padding: const EdgeInsets.symmetric(
            horizontal: QSpace.md, vertical: QSpace.sm),
        child: Text(text,
            style: QCueText.body.copyWith(color: context.q.text3)),
      );
}
