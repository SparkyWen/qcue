// QCue S4-R56/R64: the legacy boolean [offlineProvider] (which the screens and
// the OfflineBanner read) is now backed by the real [connectivityProvider], so
// flipping connectivity flips the banner. It stays a `Provider<bool>` so the
// existing widget tests can still override it directly.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/offline/connectivity.dart';

void main() {
  test('offlineProvider reflects the connectivity singleton', () {
    final c = ProviderContainer();
    addTearDown(c.dispose);
    expect(c.read(offlineProvider), isFalse); // online by default
    c.read(connectivityProvider.notifier).setOffline();
    expect(c.read(offlineProvider), isTrue);
    c.read(connectivityProvider.notifier).setOnline();
    expect(c.read(offlineProvider), isFalse);
  });

  test('offlineProvider remains directly overridable (test seam intact)', () {
    final c = ProviderContainer(
      overrides: [offlineProvider.overrideWithValue(true)],
    );
    addTearDown(c.dispose);
    expect(c.read(offlineProvider), isTrue);
  });
}
