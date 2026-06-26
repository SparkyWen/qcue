// QCue S1-R88 — forward-compatible event envelope crossing FFI/SSE/WSS (Master §8).
// CANONICAL: one type in `protocol`, imported (never redefined) by app-server-protocol/S3.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Forward-compat: NO `deny_unknown_fields`; `event` is a String so future kinds round-trip.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeEventEnvelope {
    pub schema_version: u32,
    #[serde(default)]
    pub thread_id: Uuid,
    #[serde(default)]
    pub turn_id: Option<Uuid>,
    #[serde(default)]
    pub seq: u64,
    /// Event kind string; unknown kinds are skipped by old consumers (forward-compat).
    pub event: String,
    /// Forward-compat: an omitted `payload` deserializes to JSON `null` (Value::default) so a
    /// minimal envelope from an older/newer client still round-trips.
    #[serde(default)]
    pub payload: Value,
}

/// Optional typed helper for CONSTRUCTING/MATCHING known event kinds. The WIRE shape
/// always carries `event: String` (above); this enum never changes serialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeEvent {
    MessageStart,
    ContentBlockStart,
    ContentBlockDelta,
    ContentBlockStop,
    MessageDelta,
    MessageStop,
}
