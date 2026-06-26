// QCue S1-R20 — Anthropic-shaped internal stream model. N producers, one taxonomy.
use crate::error::ApiError;
use crate::response::FinishReason;
use crate::usage::CanonicalUsage;
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum Block {
    Text,
    Thinking,
    ToolUse { id: String, name: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum Delta {
    TextDelta(String),
    ThinkingDelta(String),
    InputJsonDelta { partial_json: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum StreamEvent {
    MessageStart,
    ContentBlockStart(Block),
    ContentBlockDelta(Delta),
    ContentBlockStop,
    MessageDelta { stop_reason: Option<FinishReason>, usage: Option<CanonicalUsage> },
    MessageStop,
}

pub type StreamEventBox = Pin<Box<dyn Stream<Item = Result<StreamEvent, ApiError>> + Send + 'static>>;
