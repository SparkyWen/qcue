// QCue: presentation helpers for the Intelligence + Model selector. Pure, dependency-free
// so they're trivially unit-testable. The backend hands us bare model ids (e.g. `gpt-5.5`);
// these humanize them and define the 5 ChatGPT-style "Intelligence" levels mapped onto the
// existing RecallEffort wire tokens.
import 'recall_selection.dart';

/// Curated display names for the catalog (latest flagship + one budget model per provider).
/// Anything not listed falls back to a title-cased form.
const Map<String, String> _modelDisplayOverrides = {
  'gpt-5.5': 'GPT-5.5',
  'gpt-5.4-mini': 'GPT-5.4 mini',
  'claude-opus-4-8': 'Claude Opus 4.8',
  'claude-haiku-4-5': 'Claude Haiku 4.5',
  'gemini-3-pro': 'Gemini 3 Pro',
  'gemini-3-flash': 'Gemini 3 Flash',
  'deepseek-v4-pro': 'DeepSeek V4 Pro',
  'deepseek-v4-flash': 'DeepSeek V4 Flash',
};

/// Humanize a bare model id into a display name (e.g. `gpt-5.5` → `GPT-5.5`).
String modelDisplayName(String id) {
  final hit = _modelDisplayOverrides[id];
  if (hit != null) return hit;
  // Fallback: split on separators, capitalize each word, join with spaces.
  return id
      .split(RegExp(r'[-_]'))
      .where((w) => w.isNotEmpty)
      .map((w) => w[0].toUpperCase() + w.substring(1))
      .join(' ');
}

/// One ChatGPT-style Intelligence level: a display label bound to a backend effort.
class IntelligenceLevel {
  const IntelligenceLevel(this.label, this.effort);
  final String label;
  final RecallEffort effort;
}

/// The 5 levels shown in the selector, mapped onto the backend's effort tokens.
/// (QCue's `low` tier is intentionally not surfaced — the 5 ChatGPT labels are the UX.)
const List<IntelligenceLevel> intelligenceLevels = [
  IntelligenceLevel('Instant', RecallEffort.minimal),
  IntelligenceLevel('Medium', RecallEffort.medium),
  IntelligenceLevel('High', RecallEffort.high),
  IntelligenceLevel('Extra High', RecallEffort.xHigh),
  IntelligenceLevel('Pro', RecallEffort.max),
];

/// The label shown for a selected effort (or `Default` when none is set).
String intelligenceLabelFor(RecallEffort? effort) {
  if (effort == null) return 'Default';
  for (final l in intelligenceLevels) {
    if (l.effort == effort) return l.label;
  }
  return effort.label; // efforts outside the surfaced 5 fall back to their own label
}

/// The compact pill text in the recall composer, e.g. `GPT-5.5 · Instant`,
/// `Auto · High`, or `Auto · Default`.
String selectorPillLabel(RecallSelection sel) {
  final model = sel.model != null ? modelDisplayName(sel.model!) : 'Auto';
  return '$model · ${intelligenceLabelFor(sel.effort)}';
}
