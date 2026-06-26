// QCue S1-R85..R88 — the pure FFI core (unit-tested). The flutter_rust_bridge-facing wrapper
// lives in `bridge.rs`; it is compiled (against the real FRB runtime) but the logic + tests live here.
//
// Two-egress design (S1-R86): the WSS/SSE egress (S3) and this FFI egress share ONE event model.
// Each `StreamEvent` is mapped to a forward-compatible `RuntimeEventEnvelope` whose `payload` is the
// raw `StreamEvent` JSON — no provider-native wire shape ever crosses the boundary.
pub mod bridge;

use futures_util::StreamExt;
use protocol::{RuntimeEventEnvelope, StreamEvent, StreamEventBox};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// The canonical envelope schema version stamped on every FFI/SSE event (Master §8).
pub const SCHEMA_VERSION: u32 = 1;

/// The event kinds a v1 consumer understands. Unknown/future kinds (e.g. emitted by a newer
/// runtime) are skipped by a v1 consumer without error — that is the forward-compat guarantee.
const KNOWN_EVENTS_V1: &[&str] = &[
    "MessageStart",
    "ContentBlockStart",
    "ContentBlockDelta",
    "ContentBlockStop",
    "MessageDelta",
    "MessageStop",
];

/// S1-R88 — a v1 consumer only handles these event kinds; unknown kinds are skipped.
pub fn is_known_event_v1(kind: &str) -> bool {
    KNOWN_EVENTS_V1.contains(&kind)
}

/// Map a `StreamEvent` to its envelope (the discriminant name + the serialized payload).
/// `thread_id`/`turn_id`/`seq` are stamped by the caller (`forward_events`); the pure mapper
/// leaves them at default so the function stays a total, side-effect-free mapping (S1-R86).
pub fn envelope_for(ev: &StreamEvent) -> RuntimeEventEnvelope {
    let kind = match ev {
        StreamEvent::MessageStart => "MessageStart",
        StreamEvent::ContentBlockStart(_) => "ContentBlockStart",
        StreamEvent::ContentBlockDelta(_) => "ContentBlockDelta",
        StreamEvent::ContentBlockStop => "ContentBlockStop",
        StreamEvent::MessageDelta { .. } => "MessageDelta",
        StreamEvent::MessageStop => "MessageStop",
    };
    RuntimeEventEnvelope {
        schema_version: SCHEMA_VERSION,
        thread_id: uuid::Uuid::nil(),
        turn_id: None,
        seq: 0,
        event: kind.into(),
        // serialization of an owned enum cannot fail; fall back to Null defensively (no unwrap).
        payload: serde_json::to_value(ev).unwrap_or(serde_json::Value::Null),
    }
}

/// The egress seam. The real flutter_rust_bridge `StreamSink<RuntimeEventEnvelope>` implements this
/// (see `bridge::FrbStreamSink`); tests use the `Vec`-backed `SinkRecorder`. Keeping the surface over
/// a trait makes the core codegen-independent while the FRB runtime type still rides the public path.
pub trait EnvelopeSink: Send + Sync {
    fn add(&self, env: RuntimeEventEnvelope);
}

/// A test sink that records envelopes (stands in for the flutter_rust_bridge `StreamSink`).
#[derive(Clone, Default)]
pub struct SinkRecorder {
    inner: Arc<Mutex<Vec<RuntimeEventEnvelope>>>,
}

impl SinkRecorder {
    /// Snapshot of every envelope pushed so far. Tolerates a poisoned lock (recovers the inner Vec)
    /// so a panicking producer in another task can't make the recorder itself panic.
    pub fn events(&self) -> Vec<RuntimeEventEnvelope> {
        match self.inner.lock() {
            Ok(g) => g.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl EnvelopeSink for SinkRecorder {
    fn add(&self, env: RuntimeEventEnvelope) {
        match self.inner.lock() {
            Ok(mut g) => g.push(env),
            Err(poisoned) => poisoned.into_inner().push(env),
        }
    }
}

/// S1-R85/R87 — forward each `StreamEvent` to the sink as an envelope; `cancel` stops forwarding.
/// Stamps a monotonic 0-based `seq` onto each envelope so consumers can order/dedup. A pre-cancelled
/// token (or one tripped mid-stream) ends forwarding immediately — that is how the Dart UI's stop
/// button crosses the FFI boundary (S1-R87).
pub async fn forward_events<S: EnvelopeSink>(
    mut stream: StreamEventBox,
    sink: S,
    cancel: CancellationToken,
) {
    let mut seq: u64 = 0;
    loop {
        if cancel.is_cancelled() {
            return;
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            item = stream.next() => match item {
                Some(Ok(ev)) => {
                    let mut env = envelope_for(&ev);
                    env.seq = seq;
                    seq += 1;
                    sink.add(env);
                }
                // A scripted/transport error or end-of-stream both end forwarding. The error path is
                // surfaced to the UI via the turn's TurnResult, not as a partial envelope here.
                Some(Err(_)) | None => return,
            }
        }
    }
}
