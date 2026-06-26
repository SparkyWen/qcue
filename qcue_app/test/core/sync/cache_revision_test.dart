// QCue: the cache-revision signal must drive the read-providers to RE-READ. This pins the fix for
// the reported "digest/recall results only show after I restart the app" staleness and the
// first-open-blank: a cold provider that resolved against an empty cache must re-resolve (without
// recreating the container) once a sync snapshot bumps the revision.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/models/recall_conversation.dart';
import 'package:qcue_app/core/models/screen_state.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/core/sync/cache_revision.dart';
import 'package:qcue_app/features/capture/capture_provider.dart';
import 'package:qcue_app/features/recall/conversations_provider.dart';
import 'package:qcue_app/features/wiki/wiki_provider.dart';

/// A fake whose first read of each surface is EMPTY (the cold cache) and every later read returns the
/// seeded data (the warm cache after a sync snapshot). Only the read methods are needed; everything
/// else routes to noSuchMethod (never called by these tests).
class _ColdThenWarmApi implements QcueApiClient {
  final _seed = StubApiClient.seeded();
  int captureReads = 0;
  int wikiReads = 0;
  int convoReads = 0;

  @override
  Future<List<Idea>> captures({DateTime? day}) async {
    captureReads++;
    return captureReads <= 1 ? <Idea>[] : _seed.captures();
  }

  @override
  Future<List<WikiPage>> wikiIndex() async {
    wikiReads++;
    return wikiReads <= 1 ? <WikiPage>[] : _seed.wikiIndex();
  }

  @override
  Future<List<ConversationSummary>> listConversations() async {
    convoReads++;
    return convoReads <= 1 ? <ConversationSummary>[] : _seed.listConversations();
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test('captureFeedProvider re-reads the cache when the revision bumps (RC1/RC4)', () async {
    final api = _ColdThenWarmApi();
    final c = ProviderContainer(overrides: [apiClientProvider.overrideWithValue(api)]);
    addTearDown(c.dispose);
    // keep the provider alive so its ref.watch(cacheRevision) stays subscribed.
    final sub = c.listen(captureFeedProvider, (_, __) {});
    addTearDown(sub.close);

    final cold = await c.read(captureFeedProvider.future);
    expect(cold, isA<Empty>(), reason: 'first-open resolves against the empty cold cache');

    // a sync pull landed fresh data; bumping must re-read the (now warm) cache.
    c.read(cacheRevisionProvider.notifier).bump();
    await Future<void>.delayed(Duration.zero);

    final warm = await c.read(captureFeedProvider.future);
    expect(warm, isA<Data<List<Idea>>>(), reason: 'blank feed must self-heal on bump, no relaunch');
    expect(api.captureReads, 2, reason: 'the provider must have actually re-read');
  });

  test('wikiIndexProvider re-reads when the revision bumps (RC2 digest surfacing)', () async {
    final api = _ColdThenWarmApi();
    final c = ProviderContainer(overrides: [apiClientProvider.overrideWithValue(api)]);
    addTearDown(c.dispose);
    final sub = c.listen(wikiIndexProvider, (_, __) {});
    addTearDown(sub.close);

    expect(await c.read(wikiIndexProvider.future), isA<Empty>());
    c.read(cacheRevisionProvider.notifier).bump();
    await Future<void>.delayed(Duration.zero);
    expect(await c.read(wikiIndexProvider.future), isA<Data<List<WikiPage>>>());
  });

  test('conversationsProvider re-reads when the revision bumps (RC3 recall done)', () async {
    final api = _ColdThenWarmApi();
    final c = ProviderContainer(overrides: [apiClientProvider.overrideWithValue(api)]);
    addTearDown(c.dispose);
    final sub = c.listen(conversationsProvider, (_, __) {});
    addTearDown(sub.close);

    expect(await c.read(conversationsProvider.future), isA<Empty>());
    c.read(cacheRevisionProvider.notifier).bump();
    await Future<void>.delayed(Duration.zero);
    expect(await c.read(conversationsProvider.future), isA<Data<List<ConversationSummary>>>());
  });
}
