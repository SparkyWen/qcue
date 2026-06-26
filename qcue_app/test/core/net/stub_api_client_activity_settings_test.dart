// QCue S4: the Activity + Settings screens read/write through the SAME
// QcueApiClient seam (extended here). These tests pin the new fixture contract:
// pending wiki_merge/wiki_delete candidates, a scripted + finished dream, recent
// jobs spanning every job_state, a today's-cost figure, the masked-key vault
// (putKey returns ONLY a key_hint, never the secret), the model list, and the
// server-Dream (D9) privacy posture.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/models/sse_event.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';

void main() {
  test('approvals() returns pending wiki_merge + wiki_delete candidates',
      () async {
    final api = StubApiClient.seeded();
    final pending = await api.approvals();
    expect(pending, isNotEmpty);
    expect(pending.every((a) => a.status == ApprovalStatus.pending), isTrue);
    final actions = pending.map((a) => a.action).toSet();
    expect(actions, containsAll(<String>{'wiki_merge', 'wiki_delete'}));
    // each carries a human summary + a target slug for the candidate row
    expect(pending.every((a) => a.subjectRef['summary'] is String), isTrue);
    expect(pending.every((a) => a.subjectRef['target_slug'] is String), isTrue);
  });

  test('respondApproval(approve) resolves the candidate (drops from pending)',
      () async {
    final api = StubApiClient.seeded();
    final before = await api.approvals();
    final id = before.first.id;
    await api.respondApproval(id, true);
    final after = await api.approvals();
    expect(after.map((a) => a.id), isNot(contains(id)));
    expect(after, hasLength(before.length - 1));
  });

  test('respondApproval(reject) also resolves the candidate', () async {
    final api = StubApiClient.seeded();
    final before = await api.approvals();
    final id = before.first.id;
    await api.respondApproval(id, false);
    final after = await api.approvals();
    expect(after.map((a) => a.id), isNot(contains(id)));
  });

  test('jobs() returns recent rows spanning multiple job_states', () async {
    final api = StubApiClient.seeded();
    final jobs = await api.jobs();
    expect(jobs, isNotEmpty);
    final states = jobs.map((j) => j.state).toSet();
    expect(states.length, greaterThan(1));
    // a running dream is present so the live card has something to mount
    expect(jobs.any((j) => j.kind == JobKind.dream), isTrue);
  });

  test('todayCostMicros() is a non-negative figure', () async {
    final api = StubApiClient.seeded();
    expect(await api.todayCostMicros(), greaterThanOrEqualTo(0));
  });

  test('dreamEvents emits a scripted progress sequence then completes',
      () async {
    final api = StubApiClient.seeded();
    final events = await api.dreamEvents('d-running').toList();
    expect(events.first, isA<DreamStarted>());
    expect(events.whereType<DreamProgress>().length, greaterThan(1));
    expect(events.last, isA<DreamCompleted>());
    // the last 6 / collapse discipline needs >6 turns to exercise
    expect(events.whereType<DreamProgress>().length, greaterThan(6));
  });

  test('credentials() returns masked-hint creds spanning health states',
      () async {
    final api = StubApiClient.seeded();
    final creds = await api.credentials();
    expect(creds, isNotEmpty);
    // every cred exposes ONLY a key_hint (never a secret-shaped string)
    expect(creds.every((c) => c.keyHint.isNotEmpty), isTrue);
    expect(creds.any((c) => c.keyHint.contains('…')), isTrue);
    final states = creds.map((c) => c.status).toSet();
    expect(states.length, greaterThan(1));
  });

  test('putKey returns a ProviderCredential carrying ONLY a key_hint', () async {
    final api = StubApiClient.seeded();
    final cred = await api.putKey('openai', 'sk-live-secret-ABCD');
    expect(cred.provider, 'openai');
    expect(cred.keyHint, contains('ABCD')); // last-4 surfaced
    // the secret never round-trips out of the seam
    expect(cred.toJson().toString(), isNot(contains('secret')));
    expect(cred.toJson().toString(), isNot(contains('sk-live')));
    // it lands in the vault listing, still masked
    final creds = await api.credentials();
    final openai = creds.firstWhere((c) => c.provider == 'openai');
    expect(openai.keyHint, contains('ABCD'));
  });

  test('deleteKey removes the provider from the vault listing', () async {
    final api = StubApiClient.seeded();
    await api.putKey('openai', 'sk-AAAA');
    await api.deleteKey('openai');
    final creds = await api.credentials();
    expect(creds.map((c) => c.provider), isNot(contains('openai')));
  });

  test('costLedger() returns pre-aggregated per-day rows', () async {
    final api = StubApiClient.seeded();
    final rows = await api.costLedger();
    expect(rows, isNotEmpty);
    expect(rows.every((r) => r.costMicros >= 0), isTrue);
  });

  test('fetchModels(provider) returns a non-empty model list', () async {
    final api = StubApiClient.seeded();
    final models = await api.fetchModels('openai');
    expect(models, isNotEmpty);
  });

  test('activeModel / setActiveModel round-trip', () async {
    final api = StubApiClient.seeded();
    final models = await api.fetchModels('openai');
    await api.setActiveModel('openai', models.last);
    expect(await api.activeModel('openai'), models.last);
  });

  test('serverDream / setServerDream control the D9 posture', () async {
    final api = StubApiClient.seeded();
    final initial = await api.serverDream();
    await api.setServerDream(!initial);
    expect(await api.serverDream(), !initial);
  });
}
