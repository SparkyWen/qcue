// QCue S4-R12: no raw color literals outside core/theme — semantic tokens only.
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('S4-R12: no Color(0x..) or Colors.* outside core/theme', () {
    final hex = RegExp(r'Color\(0x|Colors\.');
    final offenders = <String>[];
    for (final e in Directory('lib').listSync(recursive: true)) {
      if (e is! File || !e.path.endsWith('.dart')) continue;
      // Normalize separators so the core/theme exclusion holds on Windows
      // (paths come back with `\`) as well as POSIX hosts.
      final p = e.path.replaceAll(r'\', '/');
      if (p.contains('/core/theme/')) continue;
      if (hex.hasMatch(e.readAsStringSync())) offenders.add(e.path);
    }
    expect(
      offenders,
      isEmpty,
      reason: 'raw color literals outside core/theme: $offenders',
    );
  });
}
