// QCue: the Intelligence selector's presentation helpers.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/features/recall/model_display.dart';
import 'package:qcue_app/features/recall/recall_selection.dart';

void main() {
  test('humanizes catalog model ids; title-cases unknowns', () {
    expect(modelDisplayName('gpt-5.5'), 'GPT-5.5');
    expect(modelDisplayName('claude-opus-4-8'), 'Claude Opus 4.8');
    expect(modelDisplayName('deepseek-v4-pro'), 'DeepSeek V4 Pro');
    expect(modelDisplayName('something-new'), 'Something New');
  });

  test('the 5 ChatGPT levels map to the right wire tokens', () {
    expect(intelligenceLevels.map((l) => l.label).toList(),
        ['Instant', 'Medium', 'High', 'Extra High', 'Pro']);
    expect(intelligenceLevels.map((l) => l.effort.wire).toList(),
        ['minimal', 'medium', 'high', 'xhigh', 'max']);
  });

  test('labels + pill text', () {
    expect(intelligenceLabelFor(RecallEffort.high), 'High');
    expect(intelligenceLabelFor(RecallEffort.max), 'Pro');
    expect(intelligenceLabelFor(null), 'Default');
    expect(
        selectorPillLabel(const RecallSelection(
            provider: 'openai', model: 'gpt-5.5', effort: RecallEffort.minimal)),
        'GPT-5.5 · Instant');
    expect(selectorPillLabel(const RecallSelection()), 'Auto · Default');
  });
}
