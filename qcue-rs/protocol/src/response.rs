// QCue S1-R17 — minimal shared response surface; vendor state via provider_data only.
use crate::message::ToolCall;
use crate::usage::CanonicalUsage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NormalizedResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub finish_reason: FinishReason,
    pub reasoning: Option<String>,
    pub usage: Option<CanonicalUsage>,
    #[ts(type = "any")]
    pub provider_data: Option<Value>,
}
