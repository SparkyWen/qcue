// Task 14c (LOC-R1..R4): the capture funnel auto-fetches an action-time GPS fix
// ONLY when the device-local toggle is on, attaches it to BOTH the queued row and
// the immediate POST, and — critically — a NULL fix (toggle off / permission
// denied / timeout) NEVER blocks the capture. These tests drive the funnel via a
// fake LocationService (never the OS), mirroring offline_api_client_test.dart's
// construction (a seeded StubApiClient inner + an in-memory IdeaCache).
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/location/location_service.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';
import 'package:qcue_app/core/offline/offline_api_client.dart';

/// A fake LocationService returning a fixed fix (or null), never touching the OS.
class _FakeLoc implements LocationService {
  _FakeLoc(this._fix);
  final CaptureLocation? _fix;
  int calls = 0;
  @override
  Future<CaptureLocation?> currentFix(
      {Duration timeout = const Duration(seconds: 10)}) async {
    calls++;
    return _fix;
  }
}

OfflineAwareApiClient _build({
  required LocationService loc,
  required bool enabled,
}) =>
    OfflineAwareApiClient(
      StubApiClient.seeded(),
      IdeaCache(InMemoryCacheStore(), feedCap: 100),
      locationService: loc,
      locationEnabled: () => enabled,
    );

void main() {
  test('toggle ON: the fetched fix is attached to the captured idea', () async {
    final loc = _FakeLoc(const CaptureLocation(lat: 37.5, lng: -122.3, accuracyM: 8));
    final api = _build(loc: loc, enabled: true);

    final idea = await api.capture(body: 'with location', origin: 'manual');

    expect(loc.calls, 1, reason: 'a fix is fetched when the toggle is on');
    expect(idea.lat, 37.5);
    expect(idea.lng, -122.3);
    expect(idea.locAccuracyM, 8);
    // The fix also rode onto the durable local row (the funnel attaches it to
    // enqueueCapture before the network, so an offline capture keeps its fix).
    final cached = api.cache.feed().firstWhere((i) => i.body == 'with location');
    expect(cached.lat, 37.5);
    expect(cached.lng, -122.3);
    expect(cached.locAccuracyM, 8);
  });

  test('toggle OFF: no fix is fetched; location is null', () async {
    final loc = _FakeLoc(const CaptureLocation(lat: 1, lng: 2, accuracyM: 5));
    final api = _build(loc: loc, enabled: false);

    final idea = await api.capture(body: 'no location', origin: 'manual');

    expect(loc.calls, 0, reason: 'the service is never called when the toggle is off');
    expect(idea.lat, isNull);
    expect(idea.lng, isNull);
    expect(idea.locAccuracyM, isNull);
  });

  test('denied (null fix): the capture STILL succeeds with null location', () async {
    final loc = _FakeLoc(null); // permission denied / timeout / no fix
    final api = _build(loc: loc, enabled: true);

    final idea = await api.capture(body: 'denied location', origin: 'manual');

    expect(loc.calls, 1);
    expect(idea.body, 'denied location'); // the capture went through
    expect(idea.lat, isNull);
    expect(idea.lng, isNull);
    expect(idea.locAccuracyM, isNull);
  });

  test('accuracy 0.0 (unknown) maps to null', () async {
    // geolocator returns 0.0 when accuracy is unknown — the funnel treats it as null.
    final loc = _FakeLoc(const CaptureLocation(lat: 10, lng: 20, accuracyM: 0.0));
    final api = _build(loc: loc, enabled: true);

    final idea = await api.capture(body: 'zero accuracy', origin: 'manual');

    expect(idea.lat, 10);
    expect(idea.lng, 20);
    expect(idea.locAccuracyM, isNull, reason: '0.0 accuracy is unknown → null');
  });

  test('an explicit caller-supplied location bypasses the funnel fetch', () async {
    // A path that already knows the location (e.g. an offline flush replay) must
    // NOT trigger a fresh GPS fetch — the explicit value wins.
    final loc = _FakeLoc(const CaptureLocation(lat: 99, lng: 99, accuracyM: 99));
    final api = _build(loc: loc, enabled: true);

    final idea = await api.capture(
      body: 'explicit',
      origin: 'manual',
      lat: 1.5,
      lng: 2.5,
      accuracyM: 3.5,
    );

    expect(loc.calls, 0, reason: 'explicit lat/lng skips the funnel fetch');
    expect(idea.lat, 1.5);
    expect(idea.lng, 2.5);
    expect(idea.locAccuracyM, 3.5);
  });
}
