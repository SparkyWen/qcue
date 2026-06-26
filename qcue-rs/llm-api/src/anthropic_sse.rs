// QCue S1-R20, S1-R77 — native Anthropic SSE → internal StreamEvent (already the canonical shape).
use crate::usage_norm::usage_from_anthropic;
use futures_util::{Stream, StreamExt};
use http::sse::{SseError, SseFrame};
use protocol::{ApiError, Block, Delta, FinishReason, StreamEvent};
use serde_json::Value;
use std::pin::Pin;

type FrameStream = Pin<Box<dyn Stream<Item = Result<SseFrame, SseError>> + Send>>;

fn map_stop(raw: &str) -> FinishReason {
    match raw {
        "tool_use" => FinishReason::ToolCalls,
        "max_tokens" => FinishReason::Length,
        "refusal" => FinishReason::ContentFilter,
        _ => FinishReason::Stop,
    }
}

pub fn parse_anthropic_sse(
    mut frames: FrameStream,
) -> impl Stream<Item = Result<StreamEvent, ApiError>> + Send {
    async_stream::stream! {
        let mut started = false;
        while let Some(frame) = frames.next().await {
            let frame = match frame { Ok(f) => f, Err(e) => { yield Err(ApiError::Transport(e.to_string())); return; } };
            let kind = frame.event.as_deref().unwrap_or("");
            let v: Value = serde_json::from_str(&frame.data).unwrap_or(Value::Null);
            match kind {
                "message_start" => { if !started { yield Ok(StreamEvent::MessageStart); started = true; } }
                "content_block_start" => {
                    if !started { yield Ok(StreamEvent::MessageStart); started = true; } // tolerate missing message_start
                    let cb = v.get("content_block");
                    let ty = cb.and_then(|c| c.get("type")).and_then(|x| x.as_str()).unwrap_or("text");
                    let block = match ty {
                        "thinking" => Block::Thinking,
                        "tool_use" => Block::ToolUse {
                            id: cb.and_then(|c| c.get("id")).and_then(|x| x.as_str()).unwrap_or("toolu_0").to_string(),
                            name: cb.and_then(|c| c.get("name")).and_then(|x| x.as_str()).unwrap_or("").to_string(),
                        },
                        _ => Block::Text,
                    };
                    yield Ok(StreamEvent::ContentBlockStart(block));
                }
                "content_block_delta" => {
                    let d = v.get("delta");
                    let dty = d.and_then(|x| x.get("type")).and_then(|x| x.as_str()).unwrap_or("");
                    match dty {
                        "text_delta" => if let Some(t) = d.and_then(|x| x.get("text")).and_then(|x| x.as_str()) {
                            yield Ok(StreamEvent::ContentBlockDelta(Delta::TextDelta(t.to_string()))); },
                        "thinking_delta" => if let Some(t) = d.and_then(|x| x.get("thinking")).and_then(|x| x.as_str()) {
                            yield Ok(StreamEvent::ContentBlockDelta(Delta::ThinkingDelta(t.to_string()))); },
                        "input_json_delta" => if let Some(p) = d.and_then(|x| x.get("partial_json")).and_then(|x| x.as_str()) {
                            yield Ok(StreamEvent::ContentBlockDelta(Delta::InputJsonDelta { partial_json: p.to_string() })); },
                        // signature_delta: opaque; not surfaced as a delta — captured by the transport on replay (S1-R77).
                        _ => {}
                    }
                }
                "content_block_stop" => yield Ok(StreamEvent::ContentBlockStop),
                "message_delta" => {
                    let stop = v.get("delta").and_then(|d| d.get("stop_reason")).and_then(|x| x.as_str()).map(map_stop);
                    let usage = v.get("usage").and_then(usage_from_anthropic);
                    yield Ok(StreamEvent::MessageDelta { stop_reason: stop, usage });
                }
                "message_stop" => { yield Ok(StreamEvent::MessageStop); }
                _ => {} // unknown event kinds skipped (forward-compat)
            }
        }
    }
}
