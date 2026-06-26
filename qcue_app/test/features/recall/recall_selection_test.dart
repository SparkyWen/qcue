// QCue v0.2.2: the per-recall model+effort selection model. Locks the wire
// tokens (must match the backend Effort enum), the default, the composer label,
// and the sticky notifier's set/clear semantics.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/features/recall/recall_selection.dart';

void main() {
  group('RecallEffort', () {
    test('wire tokens match the backend Effort variants', () {
      expect(RecallEffort.minimal.wire, 'minimal');
      expect(RecallEffort.low.wire, 'low');
      expect(RecallEffort.medium.wire, 'medium');
      expect(RecallEffort.high.wire, 'high');
      expect(RecallEffort.xHigh.wire, 'xhigh');
      expect(RecallEffort.max.wire, 'max');
    });
  });

  group('RecallSelection', () {
    test('serverDefault is all-null and labels "Default"', () {
      const s = RecallSelection.serverDefault;
      expect(s.isDefault, isTrue);
      expect(s.shortLabel, 'Default');
    });
    test('shortLabel shows the model and effort', () {
      const s = RecallSelection(
          provider: 'openai', model: 'gpt-5.5', effort: RecallEffort.high);
      expect(s.shortLabel, 'gpt-5.5 · High');
    });
  });

  group('RecallSelectionNotifier', () {
    test('setModel / setEffort / clearModel / clear', () {
      final c = ProviderContainer();
      addTearDown(c.dispose);
      final n = c.read(recallSelectionProvider.notifier);

      n.setModel('anthropic', 'claude-opus-4-8');
      expect(c.read(recallSelectionProvider).provider, 'anthropic');
      expect(c.read(recallSelectionProvider).model, 'claude-opus-4-8');

      n.setEffort(RecallEffort.max);
      expect(c.read(recallSelectionProvider).effort, RecallEffort.max);
      // changing effort keeps the model.
      expect(c.read(recallSelectionProvider).model, 'claude-opus-4-8');

      n.clearModel();
      expect(c.read(recallSelectionProvider).model, isNull);
      // clearing the model keeps the effort.
      expect(c.read(recallSelectionProvider).effort, RecallEffort.max);

      n.clear();
      expect(c.read(recallSelectionProvider).isDefault, isTrue);
    });
  });
}
