// QCue S4: the Activity screen state — loads the three sections (pending
// candidates, recent jobs, today's cost) through the single apiClientProvider
// seam, and exposes the two sanctioned mutations (decide a candidate via the
// D13 confirm gate; cancel a running dream). Decisions refresh the candidate
// list so a resolved row drops.
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/models/protocol_models.dart';
import '../../core/net/api_client_provider.dart';
import '../../core/sync/cache_revision.dart';
import '../../core/theme/theme_provider.dart';
import 'activity_repository.dart';

/// Adapts the [QcueApiClient] onto the narrow [ActivityApi] mutation seam.
class _ApiActivityAdapter implements ActivityApi {
  _ApiActivityAdapter(this._api);
  final QcueApiClient _api;
  @override
  Future<void> confirmCandidate(String id, bool approve) =>
      _api.respondApproval(id, approve);
  @override
  Future<void> cancelDream(String jobId) => _api.cancelJob(jobId);
}

final activityRepositoryProvider = Provider<ActivityRepository>(
    (ref) => ActivityRepository(_ApiActivityAdapter(ref.watch(apiClientProvider))));

/// The aggregate Activity snapshot for the screen.
class ActivitySnapshot {
  const ActivitySnapshot({
    required this.candidates,
    required this.jobs,
    required this.todayCostMicros,
  });
  final List<Approval> candidates;
  final List<JobRow> jobs;
  final int todayCostMicros;

  /// The first running dream job (if any) — the live Dream card mounts off this.
  JobRow? get runningDream => jobs
      .where((j) => j.kind == JobKind.dream && j.state == JobState.leased)
      .firstOrNull;

  /// The most recent completed dream (the "Improved N pages" entry).
  JobRow? get completedDream => jobs
      .where((j) => j.kind == JobKind.dream && j.state == JobState.done)
      .firstOrNull;
}

class ActivityNotifier extends AsyncNotifier<ActivitySnapshot> {
  /// Dream job IDs already observed as `done` — so dreamCompleted() fires once
  /// per job across refreshes; the first build primes the set silently (S4-R54).
  final Set<String> _doneDreamsSeen = {};
  bool _primed = false;

  @override
  Future<ActivitySnapshot> build() async {
    ref.watch(cacheRevisionProvider); // ISO-R4: re-fetch for the new account on an account switch
    final api = ref.watch(apiClientProvider);
    final results = await Future.wait<Object>([
      api.approvals(),
      api.jobs(),
      api.todayCostMicros(),
    ]);
    final snapshot = ActivitySnapshot(
      candidates: results[0] as List<Approval>,
      jobs: results[1] as List<JobRow>,
      todayCostMicros: results[2] as int,
    );
    _detectDreamCompletion(snapshot);
    return snapshot;
  }

  /// S4-R54: fire the success haptic once when a dream job newly reaches `done`.
  /// The first build primes the seen-set silently, so opening the screen on an
  /// already-finished dream does not buzz — only a transition observed while
  /// mounted (a refresh / decide re-build) fires it.
  void _detectDreamCompletion(ActivitySnapshot snapshot) {
    final doneNow = snapshot.jobs
        .where((j) => j.kind == JobKind.dream && j.state == JobState.done)
        .map((j) => j.id)
        .toSet();
    if (!_primed) {
      _doneDreamsSeen.addAll(doneNow);
      _primed = true;
      return;
    }
    if (doneNow.difference(_doneDreamsSeen).isNotEmpty) {
      ref.read(hapticsProvider).dreamCompleted();
      _doneDreamsSeen.addAll(doneNow);
    }
  }

  /// Approve/Reject a candidate through the D13 confirm gate, then refresh so
  /// the resolved row drops from the review list. A confirm haptic fires on
  /// approve (S4-R54).
  Future<void> decideCandidate(String id, {required bool approve}) async {
    await ref.read(activityRepositoryProvider).decideCandidate(id, approve: approve);
    if (approve) ref.read(hapticsProvider).confirmed();
    state = await AsyncValue.guard(build);
  }

  Future<void> cancelDream(String jobId) async {
    await ref.read(activityRepositoryProvider).cancelDream(jobId);
    state = await AsyncValue.guard(build);
  }
}

final activityProvider =
    AsyncNotifierProvider<ActivityNotifier, ActivitySnapshot>(
        ActivityNotifier.new);
