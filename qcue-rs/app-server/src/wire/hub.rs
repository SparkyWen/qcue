//! QCue S3 — the `StreamHub`: a registry of per-stream (per-Thread / per-job) tokio `broadcast`
//! channels + per-stream replay rings. An SSE subscriber asks the hub for a `broadcast::Receiver` by
//! stream id; a producer (the recall/wiki/dream driver) `publish`es `RuntimeEventEnvelope`s, which the
//! hub both fans out to live subscribers AND records in the stream's 20-event replay ring for
//! replay-on-reconnect. Lazy channel creation so a subscribe-before-publish (the common SSE race) still
//! receives every event published after it subscribed.
use crate::wire::replay::ReplayRing;
use app_server_protocol::RuntimeEventEnvelope;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use uuid::Uuid;

/// The canonical replay-ring depth (Master §8): the last 20 events are replayable on reconnect.
pub const REPLAY_RING_CAP: usize = 20;

/// The broadcast buffer per stream. A subscriber that lags past this is `Lagged` (→ reconnect + replay).
const BROADCAST_CAP: usize = 256;

/// Hard cap on live stream entries per hub. The stream id is client-supplied (URL path), so without a
/// bound an authenticated client could subscribe to an unlimited number of distinct random UUIDs and
/// grow the registry without limit (memory DoS). At the cap we reap entries that have no live subscriber
/// (their SSE connection closed), and only then refuse a brand-new one.
const MAX_STREAMS: usize = 10_000;

struct StreamEntry {
    /// The tenant that owns this stream — set when the entry is first created (by the subscriber, which
    /// runs under its own JWT tenant). A subscribe/replay request for a stream owned by a DIFFERENT
    /// tenant is denied, so the in-process hub can't bypass RLS and leak another tenant's events.
    owner: Uuid,
    tx: broadcast::Sender<RuntimeEventEnvelope>,
    ring: ReplayRing,
    seq: u64,
}

/// A cheaply-cloneable handle to the per-stream broadcast + replay registry (lives in `AppState`).
#[derive(Clone, Default)]
pub struct StreamHub {
    inner: Arc<Mutex<HashMap<Uuid, StreamEntry>>>,
}

impl StreamHub {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Subscribe to a stream's live events, scoped to the caller's `tenant`. The channel is created on
    /// first touch (owned by `tenant`) so a subscriber that arrives before any publish still sees every
    /// later event. A request for a stream already owned by ANOTHER tenant returns a closed receiver
    /// (empty stream) — never another tenant's events. At the registry cap, idle (zero-subscriber)
    /// entries are reaped before a new stream is admitted, bounding memory.
    pub fn subscribe(&self, tenant: Uuid, stream_id: Uuid) -> broadcast::Receiver<RuntimeEventEnvelope> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = map.get_mut(&stream_id) {
            // Allow the owner; also let the FIRST subscriber claim an entry a producer created before any
            // subscribe (owner still `nil`, e.g. the dream/ingest producers publish then the client
            // connects). Once a real tenant owns it, a different tenant is denied (no cross-tenant leak).
            if entry.owner == tenant || entry.owner.is_nil() {
                entry.owner = tenant;
                return entry.tx.subscribe();
            }
            return closed_receiver(); // cross-tenant subscribe → empty stream, no leak.
        }
        if map.len() >= MAX_STREAMS {
            map.retain(|_, e| e.tx.receiver_count() > 0);
            if map.len() >= MAX_STREAMS {
                return closed_receiver(); // registry full of live streams → refuse (DoS bound).
            }
        }
        map.entry(stream_id)
            .or_insert_with(|| StreamEntry {
                owner: tenant,
                tx: broadcast::channel(BROADCAST_CAP).0,
                ring: ReplayRing::new(REPLAY_RING_CAP),
                seq: 0,
            })
            .tx
            .subscribe()
    }

    /// Allocate the next monotonic `seq` for a stream (so producers don't track it themselves). Called by
    /// the trusted server-side producer, which runs after the owning subscriber created the entry; a
    /// not-yet-subscribed stream is created owned by `nil` (only enforced on the read paths).
    pub fn next_seq(&self, stream_id: Uuid) -> u64 {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map.entry(stream_id).or_insert_with(|| StreamEntry {
            owner: Uuid::nil(),
            tx: broadcast::channel(BROADCAST_CAP).0,
            ring: ReplayRing::new(REPLAY_RING_CAP),
            seq: 0,
        });
        entry.seq += 1;
        entry.seq
    }

    /// Publish one envelope to a stream: fan out to live subscribers AND record in the replay ring.
    /// A send error (no live subscribers) is fine — the ring still backfills a later reconnect.
    pub fn publish(&self, env: RuntimeEventEnvelope) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map.entry(env.thread_id).or_insert_with(|| StreamEntry {
            owner: Uuid::nil(),
            tx: broadcast::channel(BROADCAST_CAP).0,
            ring: ReplayRing::new(REPLAY_RING_CAP),
            seq: env.seq,
        });
        entry.ring.push(env.clone());
        let _ = entry.tx.send(env);
    }

    /// Replay a stream's missed tail (`seq >= since`), scoped to `tenant`. `None` ⇒ older than the ring
    /// (→ `resync_required`) OR the stream is owned by another tenant (no cross-tenant ring leak).
    pub fn replay_since(&self, tenant: Uuid, stream_id: Uuid, since: u64) -> Option<Vec<RuntimeEventEnvelope>> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.get(&stream_id)
            // owner-scoped: the caller's tenant, or a not-yet-claimed (nil) producer entry. A reconnect
            // always `subscribe`s first (which claims a nil owner), so by here a real owner already matches.
            .filter(|e| e.owner == tenant || e.owner.is_nil())
            .and_then(|e| e.ring.since(since))
    }

    /// Drop a finished stream's channel + ring (called when a turn/job terminates).
    pub fn close(&self, stream_id: Uuid) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(&stream_id);
    }
}

/// A receiver whose sender is already dropped — `recv()` yields `Closed` immediately, so the SSE stream
/// ends cleanly with no events. Returned when a subscribe is denied (foreign tenant / registry full).
fn closed_receiver() -> broadcast::Receiver<RuntimeEventEnvelope> {
    let (_tx, rx) = broadcast::channel(1);
    rx
}
