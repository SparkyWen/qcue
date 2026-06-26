//! QCue S3-R33/R34/R35 — the JSON-RPC-lite dispatcher: the init-handshake gate, the per-connection
//! notification opt-out, and the `-32001` backpressure bound.
//!
//! The dispatcher is the per-connection decision layer that sits in front of the `Engine`: it rejects
//! any method before `initialize` (the init gate, `-32010`), rejects a repeat `initialize` (`-32011`),
//! honours the connection's `opt_out_notification_methods` list, and applies a bounded-inflight
//! backpressure (`-32001` "overloaded") so a flood of `turn/start`s can't pin the worker. The wire
//! field `event`/`method` is a forward-compat String, so unknown future methods are simply accepted
//! once initialized (the Engine routes them).
use app_server_protocol::{error_codes, Message};
use std::collections::HashSet;

/// The per-message dispatch outcome. `Ok` carries the JSON result to write back (for a request),
/// `Err(code, message)` is the JSON-RPC-lite error, and `Notify` is a fire-and-forget notification.
pub enum DispatchResult {
    Ok(serde_json::Value),
    Err(i32, String),
    Notify,
}

/// One per WSS connection. Tracks the init gate, the opt-out set, and the bounded inflight budget.
pub struct Dispatcher {
    initialized: bool,
    suppressed: HashSet<String>,
    capacity: usize,
    inflight: usize,
}

impl Dispatcher {
    /// `capacity` is the per-connection inflight bound; the `capacity+1`-th in-flight request → -32001.
    pub fn new(capacity: usize) -> Self {
        Dispatcher { initialized: false, suppressed: HashSet::new(), capacity, inflight: 0 }
    }

    /// Whether this connection opted out of a notification method (so the writer drops it).
    pub fn is_suppressed(&self, method: &str) -> bool {
        self.suppressed.contains(method)
    }

    /// Decode + route one raw JSON-RPC-lite frame, enforcing the init gate + backpressure.
    pub fn handle(&mut self, raw: &str) -> DispatchResult {
        let msg: Message = match serde_json::from_str(raw) {
            Ok(m) => m,
            Err(e) => return DispatchResult::Err(-32700, e.to_string()), // parse error
        };
        let method = match &msg {
            Message::Request(r) => r.method.clone(),
            Message::Notification(n) => n.method.clone(),
            // a Response/Error frame from the peer is not a method call — nothing to dispatch.
            _ => return DispatchResult::Notify,
        };
        if method == "initialize" {
            if self.initialized {
                return DispatchResult::Err(
                    error_codes::ALREADY_INITIALIZED,
                    "Already initialized".into(),
                );
            }
            self.initialized = true;
            if let Message::Request(r) = &msg
                && let Some(arr) =
                    r.params.get("opt_out_notification_methods").and_then(|v| v.as_array())
            {
                for m in arr {
                    if let Some(s) = m.as_str() {
                        self.suppressed.insert(s.to_string());
                    }
                }
            }
            return DispatchResult::Ok(serde_json::json!({"ok": true}));
        }
        // the init gate: no method before initialize (S3-R33).
        if !self.initialized {
            return DispatchResult::Err(error_codes::NOT_INITIALIZED, "Not initialized".into());
        }
        // S3-R34 backpressure: a bounded inflight per connection → -32001 on saturation. A slow client
        // that never `complete_one`s saturates ITS OWN budget, never another connection's.
        if self.inflight >= self.capacity {
            return DispatchResult::Err(
                error_codes::OVERLOADED,
                "Server overloaded; retry later.".into(),
            );
        }
        self.inflight += 1;
        DispatchResult::Ok(serde_json::json!({"accepted": method}))
    }

    /// Mark one inflight request complete, freeing a backpressure slot.
    pub fn complete_one(&mut self) {
        self.inflight = self.inflight.saturating_sub(1);
    }
}
