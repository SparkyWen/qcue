// Store-screenshot driver. Run with:
//   flutter drive \
//     --driver=test_driver/integration_test.dart \
//     --target=integration_test/store_screenshots_test.dart \
//     -d <simulator-udid> --dart-define=QCUE_STUB=true
//
// Each binding.takeScreenshot(name) call in the target test streams the PNG bytes
// back here; we write them to qcue_app/screenshots/<name>.png at native device
// resolution (e.g. 1320x2868 on the 6.9" iPhone simulator).
import 'dart:io';

import 'package:integration_test/integration_test_driver_extended.dart';

Future<void> main() async {
  await integrationDriver(
    onScreenshot: (String name, List<int> bytes, [Map<String, Object?>? args]) async {
      final file = File('screenshots/$name.png');
      file.parent.createSync(recursive: true);
      file.writeAsBytesSync(bytes);
      // ignore: avoid_print
      print('wrote screenshots/$name.png (${bytes.length} bytes)');
      return true;
    },
  );
}
