//! QCue S3-R37/R38 — the per-Thread 20-event replay ring for replay-on-reconnect.
//!
//! A reconnecting client carries its last-seen `seq`; the server replays the missed tail (events with
//! `seq >= since`) so the UI catches up without a full resync. The ring is bounded (20 events): if the
//! requested `since` is older than the oldest retained event, the tail is GONE and the client must do a
//! full resync (`resync_required`, `-32020`) — signalled here by `since(..) == None`.
use app_server_protocol::RuntimeEventEnvelope;
use std::collections::VecDeque;

/// The bounded replay ring (the canonical cap is 20). FIFO eviction once full.
pub struct ReplayRing {
    cap: usize,
    buf: VecDeque<RuntimeEventEnvelope>,
}

impl ReplayRing {
    /// A ring holding at most `cap` of the most recent events.
    pub fn new(cap: usize) -> Self {
        ReplayRing { cap, buf: VecDeque::with_capacity(cap) }
    }

    /// Append one event, evicting the oldest when the cap is hit.
    pub fn push(&mut self, e: RuntimeEventEnvelope) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(e);
    }

    /// Replay the events with `seq >= since`, in order. `None` ⇒ the requested seq is older than the
    /// ring's oldest retained event — the client must resync (`resync_required`, -32020).
    pub fn since(&self, since: u64) -> Option<Vec<RuntimeEventEnvelope>> {
        let oldest = self.buf.front().map(|e| e.seq).unwrap_or(0);
        if since < oldest {
            return None;
        }
        Some(self.buf.iter().filter(|e| e.seq >= since).cloned().collect())
    }

    /// How many events the ring currently holds.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}
