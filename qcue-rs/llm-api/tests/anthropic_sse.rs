// QCue S1-R20, S1-R77 — Anthropic SSE → the SAME internal StreamEvent taxonomy.
#![allow(clippy::unwrap_used)]
use futures_util::{stream, StreamExt};
use http::sse::SseFrame;
use llm_api::anthropic_sse::parse_anthropic_sse;
use protocol::{Block, Delta, StreamEvent};

fn ev(event: &str, data: &str) -> SseFrame {
    SseFrame { event: Some(event.into()), data: data.into() }
}

#[tokio::test]
async fn test_anthropic_text_and_tool() {
    let s = stream::iter(vec![
        Ok::<_, http::sse::SseError>(ev("message_start", r#"{"type":"message_start"}"#)),
        Ok(ev("content_block_start", r#"{"index":0,"content_block":{"type":"text"}}"#)),
        Ok(ev(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"text_delta","text":"Hi"}}"#,
        )),
        Ok(ev("content_block_stop", r#"{"index":0}"#)),
        Ok(ev(
            "content_block_start",
            r#"{"index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"f"}}"#,
        )),
        Ok(ev(
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"a\":1}"}}"#,
        )),
        Ok(ev("content_block_stop", r#"{"index":1}"#)),
        Ok(ev(
            "message_delta",
            r#"{"delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":5,"output_tokens":2}}"#,
        )),
        Ok(ev("message_stop", r#"{"type":"message_stop"}"#)),
    ]);
    let evs: Vec<StreamEvent> =
        parse_anthropic_sse(Box::pin(s)).map(|r| r.unwrap()).collect::<Vec<_>>().await;
    assert!(matches!(evs.first().unwrap(), StreamEvent::MessageStart));
    assert!(evs.iter().any(|e| matches!(e, StreamEvent::ContentBlockStart(Block::Text))));
    assert!(evs
        .iter()
        .any(|e| matches!(e, StreamEvent::ContentBlockDelta(Delta::TextDelta(t)) if t == "Hi")));
    assert!(evs.iter().any(
        |e| matches!(e, StreamEvent::ContentBlockStart(Block::ToolUse { id, name }) if id == "toolu_1" && name == "f")
    ));
    assert!(evs.iter().any(
        |e| matches!(e, StreamEvent::ContentBlockDelta(Delta::InputJsonDelta { partial_json }) if partial_json == "{\"a\":1}")
    ));
    assert!(matches!(evs.last().unwrap(), StreamEvent::MessageStop));
}

#[tokio::test]
async fn test_anthropic_thinking_signature_captured() {
    // S1-R77 capture-side: a thinking delta + signature_delta are surfaced as ThinkingDelta events;
    // the opaque signature is replayed by the transport (Task 17), not normalized here.
    let s = stream::iter(vec![
        Ok::<_, http::sse::SseError>(ev(
            "content_block_start",
            r#"{"index":0,"content_block":{"type":"thinking"}}"#,
        )),
        Ok(ev(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"thinking_delta","thinking":"reason"}}"#,
        )),
        Ok(ev(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"signature_delta","signature":"OPAQUE=="}}"#,
        )),
        Ok(ev("content_block_stop", r#"{"index":0}"#)),
        Ok(ev("message_stop", r#"{}"#)),
    ]);
    let evs: Vec<StreamEvent> =
        parse_anthropic_sse(Box::pin(s)).map(|r| r.unwrap()).collect::<Vec<_>>().await;
    assert!(evs.iter().any(|e| matches!(e, StreamEvent::ContentBlockStart(Block::Thinking))));
    assert!(evs
        .iter()
        .any(|e| matches!(e, StreamEvent::ContentBlockDelta(Delta::ThinkingDelta(t)) if t == "reason")));
}
