// QCue D19 / RESP-R8 — OpenAI **/v1/responses** typed SSE → internal Anthropic-shaped StreamEvent.
// Responses streams ~40 `type`-discriminated events (no `[DONE]` sentinel; the terminal is
// response.completed/.failed/.incomplete). They map onto the existing StreamEvent enum WITHOUT new
// variants. Tool name+call_id arrive WHOLE in `response.output_item.added.item` (authoritative — we read
// them there, not from the arg-delta events); arg fragments stream as response.function_call_arguments.delta;
// reasoning SUMMARY streams as response.reasoning_summary_text.delta (raw CoT is never streamed).
use crate::usage_norm::usage_from_chat;
use futures_util::{Stream, StreamExt};
use http::sse::{SseError, SseFrame};
use protocol::{ApiError, Block, Delta, FinishReason, StreamEvent};
use serde_json::Value;
use std::pin::Pin;

type FrameStream = Pin<Box<dyn Stream<Item = Result<SseFrame, SseError>> + Send>>;

#[derive(PartialEq)]
enum Open {
    None,
    Text,
    Thinking,
    Tool,
}

/// Map a Responses `incomplete_details.reason` to a finish reason (guarded — gpt-5 sometimes returns it
/// present-but-empty).
fn map_incomplete(reason: &str) -> FinishReason {
    match reason {
        "max_output_tokens" => FinishReason::Length,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Stop,
    }
}

/// Does this response object's `output` array contain a function_call item? (Structural tool-call signal —
/// Responses has no `finish_reason:"tool_calls"`.)
fn output_has_function_call(resp: &Value) -> bool {
    resp.get("output")
        .and_then(|o| o.as_array())
        .map(|a| a.iter().any(|i| i.get("type").and_then(|t| t.as_str()) == Some("function_call")))
        .unwrap_or(false)
}

pub fn parse_responses_sse(
    mut frames: FrameStream,
) -> impl Stream<Item = Result<StreamEvent, ApiError>> + Send {
    async_stream::stream! {
        let mut started = false;
        let mut open = Open::None;
        let mut saw_function_call = false;

        while let Some(frame) = frames.next().await {
            let frame = match frame { Ok(f) => f, Err(e) => { yield Err(ApiError::Transport(e.to_string())); return; } };
            let v: Value = serde_json::from_str(&frame.data).unwrap_or(Value::Null);
            // Responses sets the SSE `event:` line to the type; fall back to the JSON `type` field.
            let kind = frame.event.as_deref()
                .filter(|s| !s.is_empty())
                .or_else(|| v.get("type").and_then(|t| t.as_str()))
                .unwrap_or("");

            match kind {
                "response.created" => {
                    if !started { yield Ok(StreamEvent::MessageStart); started = true; }
                }
                "response.output_item.added" => {
                    let item = v.get("item");
                    match item.and_then(|i| i.get("type")).and_then(|t| t.as_str()).unwrap_or("") {
                        "function_call" => {
                            if open != Open::None { yield Ok(StreamEvent::ContentBlockStop); }
                            saw_function_call = true;
                            yield Ok(StreamEvent::ContentBlockStart(Block::ToolUse {
                                // call_id (the "call_…" linkage id), NOT item id ("fc_…").
                                id: item.and_then(|i| i.get("call_id")).and_then(|x| x.as_str()).unwrap_or("").to_string(),
                                name: item.and_then(|i| i.get("name")).and_then(|x| x.as_str()).unwrap_or("").to_string(),
                            }));
                            open = Open::Tool;
                        }
                        "reasoning" => {
                            if open != Open::None { yield Ok(StreamEvent::ContentBlockStop); }
                            yield Ok(StreamEvent::ContentBlockStart(Block::Thinking));
                            open = Open::Thinking;
                        }
                        _ => {} // message: text block opens on content_part.added / output_text.delta
                    }
                }
                "response.content_part.added" => {
                    let pt = v.get("part").and_then(|p| p.get("type")).and_then(|x| x.as_str()).unwrap_or("");
                    if pt == "output_text" && open != Open::Text {
                        if open != Open::None { yield Ok(StreamEvent::ContentBlockStop); }
                        yield Ok(StreamEvent::ContentBlockStart(Block::Text));
                        open = Open::Text;
                    }
                }
                "response.output_text.delta" => {
                    if open != Open::Text {
                        if open != Open::None { yield Ok(StreamEvent::ContentBlockStop); }
                        yield Ok(StreamEvent::ContentBlockStart(Block::Text));
                        open = Open::Text;
                    }
                    if let Some(d) = v.get("delta").and_then(|x| x.as_str()) {
                        yield Ok(StreamEvent::ContentBlockDelta(Delta::TextDelta(d.to_string())));
                    }
                }
                "response.output_text.done" => {
                    if open == Open::Text { yield Ok(StreamEvent::ContentBlockStop); open = Open::None; }
                }
                "response.function_call_arguments.delta" => {
                    if let Some(d) = v.get("delta").and_then(|x| x.as_str()) {
                        yield Ok(StreamEvent::ContentBlockDelta(Delta::InputJsonDelta { partial_json: d.to_string() }));
                    }
                }
                "response.output_item.done" => {
                    // Closes a tool (or reasoning) item opened by output_item.added.
                    if open == Open::Tool || open == Open::Thinking {
                        yield Ok(StreamEvent::ContentBlockStop);
                        open = Open::None;
                    }
                }
                "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                    if open != Open::Thinking {
                        if open != Open::None { yield Ok(StreamEvent::ContentBlockStop); }
                        yield Ok(StreamEvent::ContentBlockStart(Block::Thinking));
                        open = Open::Thinking;
                    }
                    if let Some(d) = v.get("delta").and_then(|x| x.as_str()) {
                        yield Ok(StreamEvent::ContentBlockDelta(Delta::ThinkingDelta(d.to_string())));
                    }
                }
                "response.reasoning_summary_text.done" => {
                    if open == Open::Thinking { yield Ok(StreamEvent::ContentBlockStop); open = Open::None; }
                }
                "response.completed" | "response.incomplete" => {
                    if open != Open::None { yield Ok(StreamEvent::ContentBlockStop); open = Open::None; }
                    let resp = v.get("response").cloned().unwrap_or(Value::Null);
                    let usage = resp.get("usage").and_then(usage_from_chat);
                    let stop = if kind == "response.incomplete" {
                        map_incomplete(resp.get("incomplete_details").and_then(|d| d.get("reason")).and_then(|r| r.as_str()).unwrap_or(""))
                    } else if saw_function_call || output_has_function_call(&resp) {
                        FinishReason::ToolCalls
                    } else {
                        FinishReason::Stop
                    };
                    yield Ok(StreamEvent::MessageDelta { stop_reason: Some(stop), usage });
                    yield Ok(StreamEvent::MessageStop);
                }
                "response.failed" => {
                    let msg = v.get("response").and_then(|r| r.get("error")).and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str()).unwrap_or("response failed").to_string();
                    yield Err(ApiError::Transport(msg));
                    return;
                }
                "error" => {
                    let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("stream error").to_string();
                    yield Err(ApiError::Transport(msg));
                    return;
                }
                _ => {} // unknown event kinds skipped (forward-compat)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use futures_util::stream;

    fn frame(event: &str, data: serde_json::Value) -> Result<SseFrame, SseError> {
        Ok(SseFrame { event: Some(event.to_string()), data: data.to_string() })
    }

    async fn collect(frames: Vec<Result<SseFrame, SseError>>) -> Vec<StreamEvent> {
        let s = stream::iter(frames);
        parse_responses_sse(Box::pin(s)).map(|r| r.unwrap()).collect::<Vec<_>>().await
    }

    #[tokio::test]
    async fn streams_a_function_call() {
        let evs = collect(vec![
            frame("response.created", serde_json::json!({"type":"response.created","response":{"id":"r"}})),
            frame("response.output_item.added", serde_json::json!({"type":"response.output_item.added","item":{"type":"function_call","id":"fc_1","call_id":"call_xyz","name":"web_search"}})),
            frame("response.function_call_arguments.delta", serde_json::json!({"type":"response.function_call_arguments.delta","delta":"{\"query\":"})),
            frame("response.function_call_arguments.delta", serde_json::json!({"type":"response.function_call_arguments.delta","delta":"\"rust\"}"})),
            frame("response.output_item.done", serde_json::json!({"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_xyz"}})),
            frame("response.completed", serde_json::json!({"type":"response.completed","response":{"id":"r","output":[{"type":"function_call","call_id":"call_xyz"}],"usage":{"input_tokens":10,"output_tokens":5}}})),
        ]).await;

        assert!(matches!(evs[0], StreamEvent::MessageStart));
        match &evs[1] {
            StreamEvent::ContentBlockStart(Block::ToolUse { id, name }) => {
                assert_eq!(id, "call_xyz", "tool block id is the call_id, not fc_id");
                assert_eq!(name, "web_search");
            }
            other => panic!("expected ToolUse start, got {other:?}"),
        }
        assert!(matches!(evs[2], StreamEvent::ContentBlockDelta(Delta::InputJsonDelta { .. })));
        assert!(matches!(evs[3], StreamEvent::ContentBlockDelta(Delta::InputJsonDelta { .. })));
        assert!(matches!(evs[4], StreamEvent::ContentBlockStop));
        match &evs[5] {
            StreamEvent::MessageDelta { stop_reason, usage } => {
                assert_eq!(*stop_reason, Some(FinishReason::ToolCalls), "function_call ⇒ ToolCalls");
                assert_eq!(usage.as_ref().unwrap().input, 10);
            }
            other => panic!("expected MessageDelta, got {other:?}"),
        }
        assert!(matches!(evs[6], StreamEvent::MessageStop));
    }

    #[tokio::test]
    async fn streams_text_and_reasoning() {
        let evs = collect(vec![
            frame("response.created", serde_json::json!({"type":"response.created","response":{"id":"r"}})),
            frame("response.output_item.added", serde_json::json!({"type":"response.output_item.added","item":{"type":"reasoning","id":"rs_1"}})),
            frame("response.reasoning_summary_text.delta", serde_json::json!({"type":"response.reasoning_summary_text.delta","delta":"thinking"})),
            frame("response.reasoning_summary_text.done", serde_json::json!({"type":"response.reasoning_summary_text.done"})),
            frame("response.content_part.added", serde_json::json!({"type":"response.content_part.added","part":{"type":"output_text"}})),
            frame("response.output_text.delta", serde_json::json!({"type":"response.output_text.delta","delta":"hello"})),
            frame("response.output_text.done", serde_json::json!({"type":"response.output_text.done"})),
            frame("response.completed", serde_json::json!({"type":"response.completed","response":{"id":"r","output":[{"type":"message"}],"usage":{"input_tokens":1,"output_tokens":1}}})),
        ]).await;

        assert!(matches!(evs[0], StreamEvent::MessageStart));
        assert!(matches!(evs[1], StreamEvent::ContentBlockStart(Block::Thinking)));
        assert!(matches!(evs[2], StreamEvent::ContentBlockDelta(Delta::ThinkingDelta(_))));
        assert!(matches!(evs[3], StreamEvent::ContentBlockStop));
        assert!(matches!(evs[4], StreamEvent::ContentBlockStart(Block::Text)));
        assert!(matches!(evs[5], StreamEvent::ContentBlockDelta(Delta::TextDelta(_))));
        assert!(matches!(evs[6], StreamEvent::ContentBlockStop));
        match &evs[7] {
            StreamEvent::MessageDelta { stop_reason, .. } => assert_eq!(*stop_reason, Some(FinishReason::Stop)),
            other => panic!("expected MessageDelta Stop, got {other:?}"),
        }
        assert!(matches!(evs[8], StreamEvent::MessageStop));
    }

    #[tokio::test]
    async fn stream_error_event_yields_err() {
        let s = stream::iter(vec![frame("error", serde_json::json!({"type":"error","message":"boom"}))]);
        let out: Vec<_> = parse_responses_sse(Box::pin(s)).collect().await;
        assert!(out[0].is_err(), "a stream-level error event must surface as Err");
    }
}
