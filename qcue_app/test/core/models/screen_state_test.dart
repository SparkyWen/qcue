import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/screen_state.dart';

String _render(ScreenState<int> s) => switch (s) {
      Loading() => 'loading',
      Empty() => 'empty',
      ErrorState(:final message) => 'error:$message',
      Data(:final value) => 'data:$value',
    };

void main() {
  test('S4-R3: switch is exhaustive over all 4 states', () {
    expect(_render(const Loading<int>()), 'loading');
    expect(_render(const Empty<int>()), 'empty');
    expect(_render(const ErrorState<int>('boom')), 'error:boom');
    expect(_render(const Data<int>(7)), 'data:7');
  });
}
