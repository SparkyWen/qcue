//! QCue S3-R37/S3-R33 — RuntimeEventEnvelope (IMPORTED from `protocol`) + JSON-RPC-lite Message + RuntimeEvent helper. Data only.
use serde::{Deserialize, Serialize};
// The canonical replay-on-reconnect wrapper is defined ONCE in the `protocol` crate (Master §8) and shared by the
// FFI egress and the WSS egress; S3 IMPORTS it (does NOT redefine). Wire shape:
//   RuntimeEventEnvelope { schema_version: u32, thread_id: Uuid, turn_id: Option<Uuid>, seq: u64, event: String, payload: serde_json::Value }
// `event` is a FORWARD-COMPATIBLE String — unknown/future kinds MUST deserialize (no `deny_unknown_fields` on the envelope).
pub use protocol::RuntimeEventEnvelope;

pub mod error_codes {
    pub const OVERLOADED: i32 = -32001;
    pub const UNAUTHORIZED: i32 = -32002;
    pub const NOT_INITIALIZED: i32 = -32010;
    pub const ALREADY_INITIALIZED: i32 = -32011;
    pub const RESYNC_REQUIRED: i32 = -32020;
    /// No usable provider credential for the turn (BYOK key missing/invalid). A CONFIG error, NOT a
    /// transient overload (-32001) and NOT a JWT auth failure (-32002) — the client must NOT retry or
    /// bounce to login; it should prompt the user to add an API key in Settings.
    pub const NO_CREDENTIALS: i32 = -32030;
    /// The tenant/user daily cost ceiling was reached BEFORE this call (D17/B-R20). A terminal,
    /// NON-retryable refusal (unlike the transient OVERLOADED -32001, which the client retries) — the
    /// client should surface "daily limit reached", not retry.
    pub const COST_CEILING: i32 = -32031;
}

// QCue S3-R37 — typed helper for CONSTRUCTING/MATCHING the KNOWN envelope `event` kinds. The wire field is a String
// (forward-compat); `RuntimeEvent::as_wire()` yields the canonical camelCase token, and unknown wire strings never error.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeEvent {
    ThreadStarted,
    TurnStarted,
    ItemStarted,
    ItemDelta,
    ItemCompleted,
    TurnCompleted,
    Usage,
    Error,
}

impl RuntimeEvent {
    /// Canonical camelCase wire token for this known event kind (the `event: String` carried on the envelope).
    pub fn as_wire(self) -> &'static str {
        match self {
            RuntimeEvent::ThreadStarted => "threadStarted",
            RuntimeEvent::TurnStarted => "turnStarted",
            RuntimeEvent::ItemStarted => "itemStarted",
            RuntimeEvent::ItemDelta => "itemDelta",
            RuntimeEvent::ItemCompleted => "itemCompleted",
            RuntimeEvent::TurnCompleted => "turnCompleted",
            RuntimeEvent::Usage => "usage",
            RuntimeEvent::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
pub struct RpcRequest {
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
pub struct RpcNotification {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
pub struct RpcResponse {
    pub id: serde_json::Value,
    pub result: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
pub struct RpcError {
    pub id: serde_json::Value,
    pub error: RpcErrorBody,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
pub struct RpcErrorBody {
    pub code: i32,
    pub message: String,
}

// JSON-RPC-lite: NO "jsonrpc":"2.0" field. Untagged → discriminated by present fields.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Message {
    Response(RpcResponse),
    Error(RpcError),
    Request(RpcRequest),
    Notification(RpcNotification),
}
