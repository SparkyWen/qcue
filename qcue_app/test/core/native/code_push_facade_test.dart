import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/native/codepush/code_push_facade.dart';

void main() {
  test('stub facade reports a scripted status', () async {
    const facade = StubCodePushFacade(currentPatch: 3, updateReady: true);
    final s = await facade.status();
    expect(s.currentPatch, 3);
    expect(s.updateReady, isTrue);
  });

  test('stub facade defaults to no patch, not ready', () async {
    const facade = StubCodePushFacade();
    final s = await facade.status();
    expect(s.currentPatch, isNull);
    expect(s.updateReady, isFalse);
  });
}
