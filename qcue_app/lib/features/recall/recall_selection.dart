// QCue v0.2.2: a per-recall model + reasoning-effort override. Users hold BYOK
// keys for several vendors; this lets them pick which provider/model and effort
// a given recall uses, to compare results. The choice is "sticky" across turns
// (a Notifier) until changed or cleared. Null fields mean "use the server/tenant
// default" — the override is opt-in and must resolve within the tenant's keys.
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Reasoning effort offered per recall. The wire token matches the Rust
/// `providers::hooks::Effort` variants (note `xHigh` → `"xhigh"`).
enum RecallEffort {
  minimal('minimal', 'Minimal'),
  low('low', 'Low'),
  medium('medium', 'Medium'),
  high('high', 'High'),
  xHigh('xhigh', 'Extra-high'),
  max('max', 'Maximum');

  const RecallEffort(this.wire, this.label);

  /// The query-param token sent to the backend.
  final String wire;

  /// The human label shown in the picker.
  final String label;
}

/// A per-recall model/effort choice. All-null = the server/tenant default.
@immutable
class RecallSelection {
  const RecallSelection({this.provider, this.model, this.effort});

  /// The BYOK provider the [model] belongs to (e.g. `openai`).
  final String? provider;

  /// The exact model id (e.g. `gpt-5.5`).
  final String? model;

  /// The reasoning-effort override.
  final RecallEffort? effort;

  /// The "let the server decide" selection.
  static const serverDefault = RecallSelection();

  bool get isDefault => provider == null && model == null && effort == null;

  /// A short composer label, e.g. `gpt-5.5 · High`, `openai`, or `Default`.
  String get shortLabel {
    if (isDefault) return 'Default';
    final parts = <String>[];
    if (model != null) {
      parts.add(model!);
    } else if (provider != null) {
      parts.add(provider!);
    }
    if (effort != null) parts.add(effort!.label);
    return parts.join(' · ');
  }
}

/// Holds the sticky recall selection. Writes are explicit (no copyWith) so an
/// effort or model can be cleared back to null deterministically.
class RecallSelectionNotifier extends Notifier<RecallSelection> {
  @override
  RecallSelection build() => RecallSelection.serverDefault;

  /// Pick a concrete provider+model (keeps the current effort).
  void setModel(String provider, String model) => state = RecallSelection(
        provider: provider,
        model: model,
        effort: state.effort,
      );

  /// Fall back to the server's default provider/model (keeps the current effort).
  void clearModel() =>
      state = RecallSelection(effort: state.effort);

  /// Pick (or clear, with null) the reasoning effort.
  void setEffort(RecallEffort? effort) => state = RecallSelection(
        provider: state.provider,
        model: state.model,
        effort: effort,
      );

  /// Reset everything to the server default.
  void clear() => state = RecallSelection.serverDefault;
}

final recallSelectionProvider =
    NotifierProvider<RecallSelectionNotifier, RecallSelection>(
        RecallSelectionNotifier.new);
