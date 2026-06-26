// QCue S4-R41/R44 / D13: the Activity write surface is exactly two RPCs — the
// candidate confirm (the candidates→confirm→canonical gate) and the Dream
// cancel (→ DreamTask.kill + clock rollback server-side, Appendix A A-R8). The
// app NEVER canonicalizes itself; it only proposes a decision (RKM §9 #18).

/// The narrow mutation seam (an adapter over [QcueApiClient] in the screen).
abstract interface class ActivityApi {
  Future<void> confirmCandidate(String id, bool approve);
  Future<void> cancelDream(String jobId);
}

/// Read-only over jobs/approvals except the two sanctioned mutations (S4-R44).
class ActivityRepository {
  ActivityRepository(this._api);
  final ActivityApi _api;

  /// Exhaustive, declared set of mutation method names (S4-R44 guard).
  Set<String> get mutationNames => {'decideCandidate', 'cancelDream'};

  Future<void> decideCandidate(String id, {required bool approve}) =>
      _api.confirmCandidate(id, approve);

  Future<void> cancelDream(String jobId) => _api.cancelDream(jobId);
}
