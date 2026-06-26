import 'package:geolocator/geolocator.dart';

/// A single positional fix attached to a capture (LOC-R1).
class CaptureLocation {
  const CaptureLocation({required this.lat, required this.lng, this.accuracyM});
  final double lat;
  final double lng;
  final double? accuracyM;
}

/// Action-time location for the capture funnel. Implementations MUST NOT throw to the funnel — any
/// failure (permission denied, service off, timeout, no fix) returns null so the capture still saves.
abstract interface class LocationService {
  Future<CaptureLocation?> currentFix({Duration timeout});
}

/// The real geolocator-backed service. Off-by-default is enforced by the CALLER (the funnel only calls
/// this when the Settings toggle is on); this class only fetches a fix when asked.
///
/// API target: geolocator 13.0.1 / geolocator_platform_interface 4.2.8
/// - [Geolocator.getCurrentPosition] accepts [locationSettings: LocationSettings(accuracy, timeLimit)]
/// - [Position.accuracy] is non-nullable double (0.0 when unavailable)
/// - [LocationPermission] values: denied, deniedForever, whileInUse, always, unableToDetermine
class GeolocatorLocationService implements LocationService {
  const GeolocatorLocationService();

  @override
  Future<CaptureLocation?> currentFix({
    Duration timeout = const Duration(seconds: 10),
  }) async {
    try {
      if (!await Geolocator.isLocationServiceEnabled()) return null;
      var perm = await Geolocator.checkPermission();
      if (perm == LocationPermission.denied) {
        perm = await Geolocator.requestPermission();
      }
      if (perm == LocationPermission.denied ||
          perm == LocationPermission.deniedForever) {
        return null;
      }
      final pos = await Geolocator.getCurrentPosition(
        locationSettings: LocationSettings(
          accuracy: LocationAccuracy.high,
          timeLimit: timeout,
        ),
      );
      return CaptureLocation(
        lat: pos.latitude,
        lng: pos.longitude,
        accuracyM: pos.accuracy,
      );
    } catch (_) {
      return null; // never block a capture on a location failure
    }
  }
}
