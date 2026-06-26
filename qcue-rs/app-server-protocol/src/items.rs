//! QCue S3-R36 — the Item taxonomy + Citation/Role. Data only.
//! `WikiEditOp`/`DreamPhase` are the canonical `protocol`-crate types (Master §8) — IMPORTED, not redefined here.
use serde::{Deserialize, Serialize};
use uuid::Uuid;
// Canonical single-definition types live in the `protocol` crate (Master §8); re-export so `Item` carries the same wire shape.
pub use protocol::{DreamPhase, WikiEditOp};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
} // D8: Owner today; enum exists so teams add variants without migration

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    pub rel_path: String,
    pub start_line: u32,
    pub end_line: u32,
}

// `WikiEditOp` is the canonical `protocol::WikiEditOp` { Create, Update, Merge { into_slug }, Delete } (serde tag="type") — imported above.
// `DreamPhase` is the canonical `protocol::DreamPhase` { Orient, Gather, Consolidate, Prune } — imported above.

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Item {
    IdeaCaptured { idea_id: Uuid, body: String },
    VoiceTranscript { idea_id: Uuid, text: String, provider: String },
    WikiEdit { page_id: Uuid, slug: String, op: WikiEditOp },
    RecallResult { answer_delta: String, citations: Vec<Citation> },
    AgentMessage { delta: String },
    DreamTurn { phase: DreamPhase, pages_touched: Vec<Uuid> },
    Reasoning { delta: String }, // D18: collapsed-by-default in app
    Error { code: i32, message: String },
}
