#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R83, S1-R84 — StubProvider: keyless/networkless deterministic fixtures + scripting.
use futures_util::StreamExt;
use protocol::{Block, Delta, FinishReason, StreamEvent};
use router::stub::{StubProvider, StubScript};

#[tokio::test]
async fn test_stub_keyless_networkless_stream() {
    let stub = StubProvider::new(StubScript::text("hello world"));
    let mut s = stub.stream();
    let mut events = Vec::new();
    while let Some(ev) = s.next().await {
        events.push(ev.unwrap());
    }
    // First is MessageStart, last is MessageStop (bracketing invariant).
    assert!(matches!(events.first().unwrap(), StreamEvent::MessageStart));
    assert!(matches!(events.last().unwrap(), StreamEvent::MessageStop));
    // Keyless: no credentials were ever read, pure in-memory (no socket opened). `network_calls`
    // counts provider invocations, so a driven stream legitimately records exactly one (S1-R84 note).
    assert_eq!(stub.network_calls(), 1);
    assert_eq!(stub.credential_reads(), 0);
}

#[tokio::test]
async fn test_stub_scripts_tool_call_and_length() {
    let stub = StubProvider::new(StubScript::tool_call("recall_search", "{\"pattern\":\"x\"}"));
    let nr = stub.complete().await.unwrap();
    let tcs = nr.tool_calls.unwrap();
    assert_eq!(tcs[0].name, "recall_search");
    assert_eq!(tcs[0].arguments, "{\"pattern\":\"x\"}");

    let stub2 = StubProvider::new(StubScript::finish(FinishReason::Length).with_text("truncat"));
    let nr2 = stub2.complete().await.unwrap();
    assert_eq!(nr2.finish_reason, FinishReason::Length);
}

#[tokio::test]
async fn test_stub_scripts_thinking_delta() {
    let stub = StubProvider::new(StubScript::thinking("reasoning…").then_text("answer"));
    let mut s = stub.stream();
    let mut saw_think = false;
    let mut saw_text = false;
    while let Some(ev) = s.next().await {
        match ev.unwrap() {
            StreamEvent::ContentBlockStart(Block::Thinking) => saw_think = true,
            StreamEvent::ContentBlockDelta(Delta::TextDelta(_)) => saw_text = true,
            _ => {}
        }
    }
    assert!(saw_think && saw_text);
}
