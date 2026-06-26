// QCue S1-R25, S1-R18 — superset transcript message + tool types.
use crate::response::FinishReason;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A tool call. `arguments` is ALWAYS a raw JSON STRING (byte-stable for the prompt cache, S1-R18).
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ToolCall {
    pub id: Option<String>,
    pub name: String,
    pub arguments: String,
    #[ts(type = "any")]
    pub provider_data: Option<Value>,
}

/// A tool definition (the dispatch seam; S2 registers `recall_search`/`read_page`/`read_lines`).
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[ts(type = "any")]
    pub input_schema: Value,
}

/// The ONE superset message; exactly one opaque `provider_data` bag (no vendor-native columns).
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Message {
    pub role: Role,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default)]
    pub finish_reason: Option<FinishReason>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    #[ts(type = "any")]
    pub provider_data: Option<Value>,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default)]
    pub is_untrusted: bool,
}

fn default_true() -> bool {
    true
}
