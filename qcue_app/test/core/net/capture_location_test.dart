// Task 14b (LOC-R1): optional location threads through the capture() seam.
// Asserts (a) the Stub stamps lat/lng/accuracy on the optimistic Idea, and
// (b) a queued capture's location SURVIVES a round-trip through the persistent
// SqliteCacheStore (the queue serializes the full Idea, so location is durable).
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';
import 'package:qcue_app/core/offline/sqlite_cache_store.dart';

void main() {
  test('StubApiClient.capture stamps the supplied location on the Idea', () async {
    final stub = StubApiClient.seeded();
    final idea = await stub.capture(
      body: 'where am I',
      origin: 'capture',
      lat: 1.0,
      lng: 2.0,
      accuracyM: 5.0,
    );
    expect(idea.lat, 1.0);
    expect(idea.lng, 2.0);
    expect(idea.locAccuracyM, 5.0);
  });

  test('a queued capture keeps its location across a store re-read', () {
    final store = SqliteCacheStore.open(':memory:');
    addTearDown(store.dispose);
    final cache = IdeaCache(store, feedCap: 100);

    final queued = cache.enqueueCapture(
      body: 'pinned thought',
      origin: 'capture',
      lat: 1.0,
      lng: 2.0,
      accuracyM: 5.0,
    );
    expect(queued.lat, 1.0);
    expect(queued.lng, 2.0);
    expect(queued.locAccuracyM, 5.0);

    // Re-read from the SAME persistent store via a fresh cache: the row must
    // still carry the location (the queue serializes the full Idea JSON).
    final reread = IdeaCache(store, feedCap: 100);
    final feedRow = reread.feed().firstWhere((i) => i.id == queued.id);
    expect(feedRow.lat, 1.0);
    expect(feedRow.lng, 2.0);
    expect(feedRow.locAccuracyM, 5.0);

    final outbound = reread.outbound().firstWhere((o) => o.idea.id == queued.id);
    expect(outbound.idea.lat, 1.0);
    expect(outbound.idea.lng, 2.0);
    expect(outbound.idea.locAccuracyM, 5.0);
  });
}
