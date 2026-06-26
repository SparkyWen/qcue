// QCue S4: the StubApiClient is the single data seam the 3 content screens
// wire to until the real WSS/SSE client lands. These tests pin its content
// contract: capture appends a `pending` idea, captures() is reverse-chrono,
// the wiki index + pages carry real [[links]]/backlinks, and recallStream
// emits a realistic scripted SSE sequence (deltas, a citation, reasoning, done).
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';

void main() {
  test('capture appends a pending idea to the front of the feed', () async {
    final api = StubApiClient.seeded();
    final before = (await api.captures()).length;
    final idea = await api.capture(body: 'a fresh thought', origin: 'capture');
    expect(idea.ingestState, IngestState.pending);
    expect(idea.body, 'a fresh thought');
    final after = await api.captures();
    expect(after, hasLength(before + 1));
    expect(after.first.id, idea.id); // reverse-chronological: newest first
  });

  test('captures() returns realistic seeded ideas, newest first', () async {
    final api = StubApiClient.seeded();
    final feed = await api.captures();
    expect(feed, isNotEmpty);
    // reverse-chronological
    for (var i = 1; i < feed.length; i++) {
      expect(
        feed[i - 1].capturedAt.isAfter(feed[i].capturedAt) ||
            feed[i - 1].capturedAt == feed[i].capturedAt,
        isTrue,
      );
    }
    // a spread of ingest states is represented (status dots have something to show)
    final states = feed.map((i) => i.ingestState).toSet();
    expect(states.contains(IngestState.ingested), isTrue);
  });

  test('wikiIndex() returns pages spanning multiple types with summaries',
      () async {
    final api = StubApiClient.seeded();
    final pages = await api.wikiIndex();
    expect(pages, isNotEmpty);
    final types = pages.map((p) => p.type).toSet();
    expect(types.length, greaterThan(1)); // grouped list has groups to show
    expect(pages.every((p) => p.summary.isNotEmpty), isTrue);
  });

  test('wikiPage(slug) returns a page whose body carries real [[links]]',
      () async {
    final api = StubApiClient.seeded();
    final page = await api.wikiPage('auto-dream');
    expect(page, isNotNull);
    expect(page!.slug, 'auto-dream');
    expect(page.bodyMarkdown, contains('[['));
    // backlinks present so the page-view Backlinks section has data
    expect(page.backlinks, isNotEmpty);
  });

  test('wikiPage(unknown) returns null (page-not-found state)', () async {
    final api = StubApiClient.seeded();
    expect(await api.wikiPage('does-not-exist'), isNull);
  });

  test('recallStream emits a realistic scripted SSE sequence', () async {
    final api = StubApiClient.seeded();
    final events = await api.recallStream('what did I decide about embeddings?')
        .toList();
    // first a session, last a done
    expect(events.first, isA<SessionStarted>());
    expect(events.last, isA<DoneEvent>());
    // streamed token-by-token: more than one message delta
    final deltas = events.whereType<MessageDelta>().toList();
    expect(deltas.length, greaterThan(1));
    // at least one citation chip + collapsible reasoning + usage
    expect(events.whereType<CitationEvent>(), isNotEmpty);
    expect(events.whereType<ReasoningDelta>(), isNotEmpty);
    expect(events.whereType<UsageEvent>(), isNotEmpty);
    // the assembled answer contains a [[wikilink]] for inline navigation
    final answer = deltas.map((d) => d.text).join();
    expect(answer, contains('[['));
  });
}
