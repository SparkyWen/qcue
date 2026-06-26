// QCue Sync Phase 1 (Task 12): the SyncClient as a Riverpod provider so the
// bootstrap can wire the real client and the connectivity host can fire `pull()`
// on the read-sync triggers (start / resume / connectivity-online / periodic).
//
// Defaults to `null` (no sync): the stub / keyless-demo path + host tests that
// don't exercise sync stay inert. The real-device bootstrap overrides it with a
// constructed [SyncClient]; sync-trigger tests override it with a spy.
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'sync_client.dart';

/// The active read-sync client, or `null` when sync is inert (stub / demo).
final syncClientProvider = Provider<SyncClient?>((_) => null);
