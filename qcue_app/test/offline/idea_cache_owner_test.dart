import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';

void main() {
  test('adoptOwner wipes when the owner differs and keeps when it matches', () {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 200);
    expect(cache.owner(), isNull);

    // First account adopts an empty cache → sets owner, no wipe.
    expect(cache.adoptOwner('userA'), isFalse);
    cache.enqueueCapture(body: 'a-secret', origin: 'test');
    expect(cache.feed(), isNotEmpty);
    expect(cache.owner(), 'userA');

    // Same account re-adopts → keeps the data.
    expect(cache.adoptOwner('userA'), isFalse);
    expect(cache.feed(), isNotEmpty);

    // Different account adopts → wipes and re-owns.
    expect(cache.adoptOwner('userB'), isTrue);
    expect(cache.feed(), isEmpty, reason: 'prior account data wiped');
    expect(cache.owner(), 'userB');
  });

  test('adoptOwner migration guard: untagged-but-populated cache is wiped on first adopt', () {
    // Simulates a pre-feature device: cache has data but no owner tag (legacy).
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 200);
    expect(cache.owner(), isNull);

    // Populate data WITHOUT calling adoptOwner — owner stays null (untagged).
    cache.enqueueCapture(body: 'residual-secret', origin: 'test');
    expect(cache.feed(), isNotEmpty, reason: 'cache has data');
    expect(cache.owner(), isNull, reason: 'owner tag is absent (pre-feature migration case)');

    // First adoptOwner call: no owner tag but data present → treat as unknown ownership → wipe.
    expect(cache.adoptOwner('userB'), isTrue, reason: 'should wipe the untagged-but-populated cache');
    expect(cache.feed(), isEmpty, reason: 'residual data from unknown account must be wiped');
    expect(cache.owner(), 'userB');
  });

  test('adoptOwner does not wipe a truly empty untagged cache', () {
    // A brand-new install: no owner, no data — should not wipe (nothing to wipe).
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 200);
    expect(cache.owner(), isNull);
    expect(cache.feed(), isEmpty);

    expect(cache.adoptOwner('userA'), isFalse, reason: 'no data to wipe; returns false');
    expect(cache.owner(), 'userA');
  });
}
