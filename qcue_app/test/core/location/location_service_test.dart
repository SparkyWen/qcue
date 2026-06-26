import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/location/location_service.dart';

class _FakeLoc implements LocationService {
  _FakeLoc(this._fix);
  final CaptureLocation? _fix;
  @override
  Future<CaptureLocation?> currentFix({Duration timeout = const Duration(seconds: 10)}) async => _fix;
}

void main() {
  test('a fix is returned when available; null is tolerated', () async {
    final ok = _FakeLoc(const CaptureLocation(lat: 1, lng: 2, accuracyM: 5));
    expect((await ok.currentFix())?.lat, 1);
    final none = _FakeLoc(null);
    expect(await none.currentFix(), isNull);
  });
}
