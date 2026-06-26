// QCue S4-R41/R44: the Activity repository is the candidate confirm RPC + Dream
// cancel RPC only. The app NEVER canonicalizes — it only proposes a decision
// (D13 candidates→confirm→canonical gate, RKM §9 #18). The only two write paths
// are decideCandidate + cancelDream.
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/features/activity/activity_repository.dart';

class FakeActivityApi implements ActivityApi {
  final calls = <String>[];
  @override
  Future<void> confirmCandidate(String id, bool approve) async =>
      calls.add('confirm:$id:$approve');
  @override
  Future<void> cancelDream(String jobId) async => calls.add('cancel:$jobId');
}

void main() {
  test('S4-R41: merge/delete candidates are destructive → Approve/Reject gate',
      () {
    const merge = Approval(
        id: 'a1',
        action: 'wiki_merge',
        status: ApprovalStatus.pending,
        requestedBy: 'dream',
        subjectRef: {});
    const delete = Approval(
        id: 'a2',
        action: 'wiki_delete',
        status: ApprovalStatus.pending,
        requestedBy: 'dream',
        subjectRef: {});
    expect(merge.isDestructive, isTrue);
    expect(delete.isDestructive, isTrue);
  });

  test('S4-R41: confirm only fires on approve, via the confirm RPC', () async {
    final api = FakeActivityApi();
    final repo = ActivityRepository(api);
    await repo.decideCandidate('a1', approve: true);
    expect(api.calls, ['confirm:a1:true']); // canonical unchanged until ack
  });

  test('S4-R41: reject routes through the same confirm RPC with approve=false',
      () async {
    final api = FakeActivityApi();
    final repo = ActivityRepository(api);
    await repo.decideCandidate('a2', approve: false);
    expect(api.calls, ['confirm:a2:false']);
  });

  test('S4-R44: cancelDream routes through the cancel RPC', () async {
    final api = FakeActivityApi();
    final repo = ActivityRepository(api);
    await repo.cancelDream('d-1');
    expect(api.calls, ['cancel:d-1']);
  });

  test('S4-R44: the only write paths are cancel and confirm', () {
    expect(ActivityRepository(FakeActivityApi()).mutationNames,
        {'decideCandidate', 'cancelDream'});
  });
}
