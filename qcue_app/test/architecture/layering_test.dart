// QCue S4-R1/R4: an import-graph lint that asserts the layering law — widgets
// and features never reach into source layers (net/ffi) directly, sources never
// import UI, and features stay isolated from each other.
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';

/// Reads every Dart file under [dir] and returns its qcue_app-internal import
/// targets, normalized to lib-relative paths (e.g. `core/net/foo.dart`). Both
/// `package:qcue_app/...` and relative (`../../core/...`) imports are resolved,
/// so the layering law cannot be bypassed by using a relative path. Pure file
/// IO so it runs without a Flutter binding.
Map<String, List<String>> _imports(String dir) {
  final out = <String, List<String>>{};
  final pkgRe = RegExp(r'''import\s+['"]package:qcue_app/([^'"]+)['"]''');
  final relRe = RegExp(r'''import\s+['"]((?:\.\./|\./)[^'"]+)['"]''');
  for (final e in Directory(dir).listSync(recursive: true)) {
    if (e is! File || !e.path.endsWith('.dart')) continue;
    final src = e.readAsStringSync();
    final deps = <String?>[
      ...pkgRe.allMatches(src).map((m) => m.group(1)),
      // Resolve relative imports against the importing file's lib-relative dir.
      for (final m in relRe.allMatches(src))
        _resolveRelative(e.path, m.group(1)!),
    ];
    out[e.path] = deps.whereType<String>().toList();
  }
  return out;
}

/// Resolves a relative import to a `lib/`-relative path (POSIX `/`), e.g. from
/// `lib/features/capture/capture_provider.dart` + `../../core/net/x.dart` →
/// `core/net/x.dart`.
String? _resolveRelative(String fromPath, String rel) {
  final fromDir = File(fromPath).parent.path.replaceAll(r'\', '/');
  final parts = fromDir.split('/')..removeWhere((p) => p.isEmpty);
  for (final seg in rel.split('/')) {
    if (seg == '..') {
      if (parts.isNotEmpty) parts.removeLast();
    } else if (seg != '.') {
      parts.add(seg);
    }
  }
  final joined = parts.join('/');
  final i = joined.indexOf('lib/');
  return i >= 0 ? joined.substring(i + 'lib/'.length) : null;
}

void main() {
  final imports = _imports('lib');

  // The single sanctioned bridge: features/widgets reach the data seam ONLY
  // through this provider, never the raw transport (qcue_api_client / the WSS/
  // SSE impl). This keeps the seam swappable without UI churn.
  const seamBridge = 'core/net/api_client_provider.dart';

  test('S4-R1: no widgets/ or features/ file imports a source layer', () {
    imports.forEach((file, deps) {
      if (file.contains('/widgets/') || file.contains('/features/')) {
        for (final d in deps) {
          if (d == seamBridge) continue; // the one allowed seam bridge
          expect(
            d.startsWith('core/net/') || d.startsWith('core/ffi/'),
            isFalse,
            reason: '$file imports source $d directly '
                '(go via the api_client_provider bridge or a repository)',
          );
        }
      }
    });
  });

  test('S4-R1: no sources/ file imports a widget', () {
    imports.forEach((file, deps) {
      if (file.startsWith('lib/core/net/') ||
          file.startsWith('lib/core/ffi/')) {
        for (final d in deps) {
          expect(
            d.startsWith('widgets/') || d.startsWith('features/'),
            isFalse,
            reason: '$file (a source) imports UI $d',
          );
        }
      }
    });
  });

  test('S4-R4: features are isolated (no cross-feature import)', () {
    imports.forEach((file, deps) {
      final m = RegExp(r'lib/features/(\w+)/').firstMatch(file);
      if (m == null) return;
      final own = m.group(1)!;
      for (final d in deps) {
        final dm = RegExp(r'features/(\w+)/').firstMatch(d);
        if (dm != null) {
          expect(
            dm.group(1),
            own,
            reason: '$file imports sibling feature $d '
                '(promote to core/widgets)',
          );
        }
      }
    });
  });
}
