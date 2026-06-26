// QCue S5-R2/R8/R11: the single capture seam every native path converges on.
// A native facade (share / widget / notification / background) NEVER touches the
// api client or the cache directly — it calls this injected [Enqueue] callback
// with a [CaptureEnqueueReq] (client-minted uuidv7 id + body + origin). The
// bootstrap binds [Enqueue] to the offline-safe, idempotent
// `OfflineAwareApiClient.capture` (persist-local-before-network, dedupe on
// retry), so every native capture is offline-safe + idempotent + records its
// `origin` for S2 fencing — with no UI/transport coupling in the facades (the
// layering law stays satisfied).
import 'native_dtos.dart';

/// The capture seam: hand a [CaptureEnqueueReq] to the local-first queue.
typedef Enqueue = Future<void> Function(CaptureEnqueueReq req);

/// A monotonic uuidv7-shaped client capture id (time-ordered; lexical sort ==
/// chronological), so a native path can tag a capture before the row exists and
/// a retry re-enqueues the SAME id (idempotent upsert, S5-R8). Mirrors the
/// offline queue's id; replaced by the Rust uuidv7 at the FFI boundary.
int _counter = 0;
String mintCaptureId() {
  final ms = DateTime.now().toUtc().millisecondsSinceEpoch;
  final seq = (_counter++) & 0xffffff;
  String hex(int v, int width) => v.toRadixString(16).padLeft(width, '0');
  final timeHi = hex((ms >> 16) & 0xffffffff, 8);
  final timeLo = hex(ms & 0xffff, 4);
  return '$timeHi-$timeLo-7${hex(seq, 6)}';
}
