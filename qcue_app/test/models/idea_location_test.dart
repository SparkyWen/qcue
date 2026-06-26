import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';

void main() {
  test('Idea round-trips location + source_page_slug', () {
    final j = {
      'id': 'i1', 'tenant_id': 't', 'user_id': 'u', 'kind': 'text', 'body': 'b',
      'origin': 'capture', 'ingest_state': 'pending', 'captured_at': '2026-06-18T00:00:00Z',
      'lat': 31.2, 'lng': 121.4, 'loc_accuracy_m': 8.0, 'source_page_slug': 'trip-src',
    };
    final idea = Idea.fromJson(j);
    expect(idea.lat, 31.2);
    expect(idea.lng, 121.4);
    expect(idea.locAccuracyM, 8.0);
    expect(idea.sourcePageSlug, 'trip-src');
    expect(idea.toJson()['lat'], 31.2);
  });

  test('Idea with no location keys: fields null + toJson omits them', () {
    final j = {
      'id': 'i2', 'tenant_id': 't', 'user_id': 'u', 'kind': 'text', 'body': 'b',
      'origin': 'capture', 'ingest_state': 'pending', 'captured_at': '2026-06-18T00:00:00Z',
    };
    final idea = Idea.fromJson(j);
    expect(idea.lat, isNull);
    expect(idea.lng, isNull);
    expect(idea.locAccuracyM, isNull);
    expect(idea.sourcePageSlug, isNull);
    final out = idea.toJson();
    expect(out.containsKey('lat'), isFalse);
    expect(out.containsKey('lng'), isFalse);
    expect(out.containsKey('loc_accuracy_m'), isFalse);
    expect(out.containsKey('source_page_slug'), isFalse);
  });
}
