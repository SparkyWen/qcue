// QCue S4-R25/R26/R28: the offline read-cache + outbound capture queue. These
// tests pin the canonical guarantees against the fast in-memory store:
//   - a capture is persisted locally (feed + queue) BEFORE any network attempt;
//   - the outbound flush is idempotent by client id (a double flush never
//     double-POSTs and never double-counts on the server);
//   - LRU eviction drops old read rows but NEVER an unflushed queued capture;
//   - a wiki read-cache holds last-opened pages for offline rendering.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';

Idea _idea(String id, String state) => Idea(
      id: id,
      tenantId: 't',
      userId: 'u',
      kind: IdeaKind.text,
      body: id,
      origin: 'capture',
      ingestState: ingestStateFromJson(state),
      capturedAt: DateTime.parse('2026-06-13T00:00:00Z'),
    );

WikiPage _page(String slug) => WikiPage(
      id: 'w-$slug',
      type: WikiPageType.concept,
      slug: slug,
      title: slug,
      summary: 's',
      bodyMarkdown: '# $slug',
      updated: DateTime.parse('2026-06-13T00:00:00Z'),
    );

void main() {
  test('S4-R25: enqueue persists locally + queues BEFORE any network', () {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 100);
    final queued = cache.enqueueCapture(body: 'hello', origin: 'capture');
    expect(queued.ingestState, IngestState.pending); // queued/pending dot
    expect(queued.id, isNotEmpty);
    expect(cache.feed().map((i) => i.id), contains(queued.id)); // in the feed
    expect(cache.outbound().map((q) => q.idea.id), contains(queued.id));
    // a fresh idempotency key (uuidv7) is stamped at enqueue time
    expect(cache.outbound().single.idempotencyKey, isNotEmpty);
    // it is marked locally queued (distinct offline state for the feed dot)
    expect(cache.isQueued(queued.id), isTrue);
  });

  test('S4-R26: flush is idempotent — same key never double-inserts', () async {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 100);
    final q = cache.enqueueCapture(body: 'a', origin: 'capture');
    final key = cache.outbound().single.idempotencyKey;
    final serverSeen = <String>[];
    Future<void> post(OutboundCapture c) async =>
        serverSeen.add(c.idempotencyKey);
    await cache.flush(post);
    await cache.flush(post); // duplicate flush (e.g. double reconnect)
    expect(serverSeen, [key]); // server saw the key exactly once
    expect(cache.outbound(), isEmpty); // acked → dequeued
    expect(cache.feed().single.ingestState, IngestState.ingested); // dot flips
    expect(cache.isQueued(q.id), isFalse); // no longer locally-queued
  });

  test('S4-R26: a failing POST keeps the capture queued for a later retry',
      () async {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 100);
    final q = cache.enqueueCapture(body: 'a', origin: 'capture');
    var attempts = 0;
    Future<void> flaky(OutboundCapture c) async {
      attempts++;
      if (attempts == 1) throw Exception('offline');
    }

    await cache.flush(flaky); // first flush throws → still queued
    expect(cache.outbound().single.idea.id, q.id);
    expect(cache.feed().single.ingestState, IngestState.pending);

    await cache.flush(flaky); // reconnect → succeeds, dequeues
    expect(cache.outbound(), isEmpty);
    expect(cache.feed().single.ingestState, IngestState.ingested);
  });

  test('S4-R28: LRU eviction drops old reads but never an unflushed capture',
      () {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 2);
    cache.putFeedRow(_idea('r1', 'ingested'));
    cache.putFeedRow(_idea('r2', 'ingested'));
    final queued = cache.enqueueCapture(body: 'q', origin: 'capture');
    cache.putFeedRow(_idea('r3', 'ingested')); // overflow → evict LRU read
    cache.putFeedRow(_idea('r4', 'ingested')); // overflow again
    final ids = cache.feed().map((i) => i.id).toSet();
    expect(ids, contains(queued.id)); // queued capture survives eviction
    expect(ids.contains('r1'), isFalse); // oldest read evicted
  });

  test('putFeed reconciles the server feed but preserves queued rows', () {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 100);
    final queued = cache.enqueueCapture(body: 'q', origin: 'capture');
    // a server refresh arrives that does NOT yet include the queued capture
    cache.putFeed([_idea('s1', 'ingested'), _idea('s2', 'ingested')]);
    final ids = cache.feed().map((i) => i.id).toSet();
    expect(ids, contains(queued.id)); // queued capture not lost on refresh
    expect(ids, containsAll(['s1', 's2']));
  });

  test('wiki read-cache stores + serves last-opened pages offline', () {
    final cache = IdeaCache(InMemoryCacheStore(), feedCap: 100, wikiCap: 2);
    cache.putWikiPage(_page('alpha'));
    cache.putWikiPage(_page('beta'));
    expect(cache.wikiPage('alpha')?.bodyMarkdown, '# alpha');
    cache.putWikiPage(_page('gamma')); // overflow → evict LRU (alpha)
    expect(cache.wikiPage('alpha'), isNull); // evicted
    expect(cache.wikiPage('beta'), isNotNull);
    expect(cache.wikiPage('gamma'), isNotNull);
    // index projection drops the body
    expect(cache.wikiIndex().every((p) => p.bodyMarkdown.isEmpty), isTrue);
  });
}
