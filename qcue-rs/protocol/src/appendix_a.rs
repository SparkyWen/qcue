// QCue S1-R1/S1-R2 — Appendix A helper types live in protocol for Dart codegen (S4-R5).
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// CANONICAL (serde tag="type"). `Merge` carries the target slug; S3/app-server-protocol IMPORTS this.
/// `schemars::JsonSchema` is derived here (additive, non-breaking) so the canonical type can be
/// embedded in `app-server-protocol::Item` (which codegens its JSON-Schema for Dart — Master §8).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, TS, schemars::JsonSchema)]
#[serde(tag = "type")]
#[ts(export)]
pub enum WikiEditOp {
    Create,
    Update,
    Merge { into_slug: String },
    Delete,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, TS, schemars::JsonSchema)]
#[ts(export)]
pub enum DreamPhase {
    Orient,
    Gather,
    Consolidate,
    Prune,
}

/// A recall/dream hit citation (App. A A-R25). `rel_path` is realpath-guarded by S2.
#[derive(Clone, Debug, Serialize, Deserialize, TS, schemars::JsonSchema)]
#[ts(export)]
pub struct Citation {
    pub rel_path: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// REC-R6/REC-D7 — one row in `GET /v1/conversations` (the recall history drawer). Serde-only.
#[derive(Clone, Debug, Serialize, Deserialize, TS, schemars::JsonSchema)]
#[ts(export)]
pub struct ConversationSummary {
    pub id: uuid::Uuid,
    pub title: String,
    /// RFC3339 UTC; the client formats it locally (the wire never carries a local clock).
    pub updated_at: String,
    #[serde(default)]
    pub last_snippet: Option<String>,
}

/// REC-R6/REC-D7 — one persisted turn in `GET /v1/conversations/{thread}/messages`. `role` is
/// `"user"|"assistant"` (no tool/system turns are persisted); `content` is the redacted final text.
#[derive(Clone, Debug, Serialize, Deserialize, TS, schemars::JsonSchema)]
#[ts(export)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
}
