import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/offline/idea_cache.dart';
import 'package:qcue_app/core/offline/sqlite_cache_store.dart';

void main() {
  test('edit then delete collapses to a single delete; cache reflects it', () async {
    final cache = IdeaCache(SqliteCacheStore.open(':memory:'), feedCap: 100);
    final i = cache.enqueueCapture(body: 'orig', origin: 'capture');
    cache.enqueueEdit(i.id, body: 'edited');
    expect(cache.feed().firstWhere((r) => r.id == i.id).body, 'edited'); // optimistic
    cache.enqueueDelete(i.id);
    expect(cache.feed().any((r) => r.id == i.id), isFalse); // optimistically removed
    final posted = <String>[];
    await cache.flushMutations((m) async => posted.add('${m.kind}:${m.id}'));
    expect(posted, ['delete:${i.id}'], reason: 'delete wins over the queued edit');
  });
}
