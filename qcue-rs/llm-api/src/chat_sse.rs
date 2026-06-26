// QCue S1-R20,R21,R24,R47,R75,R76 — OpenAI-chat SSE → internal Anthropic-shaped StreamEvent.
use crate::scrub::scrub_forged_wrappers;
use crate::usage_norm::usage_from_chat;
use futures_util::{Stream, StreamExt};
use http::sse::{SseError, SseFrame};
use protocol::{ApiError, Block, Delta, FinishReason, StreamEvent};
use serde_json::Value;
use std::collections::BTreeMap;
use std::pin::Pin;

type FrameStream = Pin<Box<dyn Stream<Item = Result<SseFrame, SseError>> + Send>>;

fn map_finish(raw: &str) -> FinishReason {
    match raw {
        "tool_calls" => FinishReason::ToolCalls,
        "length" => FinishReason::Length,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Stop,
    }
}

/// Open-block tracking so we close the right block before opening the next (S1-R20).
#[derive(PartialEq)]
enum Open {
    None,
    Text,
    Thinking,
}

pub fn parse_chat_sse(
    mut frames: FrameStream,
) -> impl Stream<Item = Result<StreamEvent, ApiError>> + Send {
    async_stream::stream! {
        yield Ok(StreamEvent::MessageStart); // S1-R21 synthetic
        let mut open = Open::None;
        // tool-call assembly keyed by `index`: (id, name, accumulated args)
        let mut tools: BTreeMap<u64, (Option<String>, Option<String>, String)> = BTreeMap::new();
        let mut tool_started: BTreeMap<u64, bool> = BTreeMap::new();
        let mut finish: Option<FinishReason> = None;
        let mut usage = None;

        while let Some(frame) = frames.next().await {
            let frame = match frame { Ok(f) => f, Err(e) => { yield Err(ApiError::Transport(e.to_string())); return; } };
            if frame.data.trim() == "[DONE]" { break; }
            let v: Value = match serde_json::from_str(&frame.data) { Ok(v) => v, Err(_) => continue }; // skip non-JSON
            if let Some(u) = v.get("usage") { usage = usage_from_chat(u); }
            let Some(choice) = v.get("choices").and_then(|c| c.get(0)) else { continue };
            let delta = choice.get("delta").cloned().unwrap_or(Value::Null);

            // reasoning_content / reasoning → Thinking
            if let Some(r) = delta.get("reasoning_content").or_else(|| delta.get("reasoning")).and_then(|x| x.as_str())
                && !r.is_empty()
            {
                if open == Open::Text { yield Ok(StreamEvent::ContentBlockStop); }
                if open != Open::Thinking { yield Ok(StreamEvent::ContentBlockStart(Block::Thinking)); open = Open::Thinking; }
                yield Ok(StreamEvent::ContentBlockDelta(Delta::ThinkingDelta(r.to_string())));
            }
            // content → Text (scrub forged wrappers, S1-R47)
            if let Some(c) = delta.get("content").and_then(|x| x.as_str())
                && !c.is_empty()
            {
                let scrubbed = scrub_forged_wrappers(c);
                if !scrubbed.is_empty() {
                    if open == Open::Thinking { yield Ok(StreamEvent::ContentBlockStop); }
                    if open != Open::Text { yield Ok(StreamEvent::ContentBlockStart(Block::Text)); open = Open::Text; }
                    yield Ok(StreamEvent::ContentBlockDelta(Delta::TextDelta(scrubbed)));
                }
            }
            // tool_calls[] keyed by index (S1-R24 reassembly, S1-R75 id fallback)
            if let Some(tcs) = delta.get("tool_calls").and_then(|x| x.as_array()) {
                for tc in tcs {
                    let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                    let entry = tools.entry(idx).or_insert((None, None, String::new()));
                    if let Some(id) = tc.get("id").and_then(|x| x.as_str()) { entry.0 = Some(id.to_string()); }
                    if let Some(name) = tc.get("function").and_then(|f| f.get("name")).and_then(|x| x.as_str()) {
                        entry.1 = Some(name.to_string());
                    }
                    if let Some(args) = tc.get("function").and_then(|f| f.get("arguments")).and_then(|x| x.as_str()) {
                        entry.2.push_str(args);
                    }
                    // open the block on first sight of a name
                    if !tool_started.get(&idx).copied().unwrap_or(false)
                        && let Some(name) = entry.1.clone()
                    {
                        if open != Open::None { yield Ok(StreamEvent::ContentBlockStop); open = Open::None; }
                        let id = entry.0.clone().unwrap_or_else(|| format!("call_{idx}"));
                        entry.0 = Some(id.clone());
                        yield Ok(StreamEvent::ContentBlockStart(Block::ToolUse { id, name }));
                        tool_started.insert(idx, true);
                    }
                }
            }
            if let Some(fr) = choice.get("finish_reason").and_then(|x| x.as_str()) {
                finish = Some(map_finish(fr));
            }
        }
        // flush tool args as one concatenated InputJsonDelta each (parsed downstream)
        for (_idx, (_id, _name, args)) in tools.into_iter() {
            if !args.is_empty() { yield Ok(StreamEvent::ContentBlockDelta(Delta::InputJsonDelta { partial_json: args })); }
        }
        if !matches!(open, Open::None) || finish.is_some() { yield Ok(StreamEvent::ContentBlockStop); }
        yield Ok(StreamEvent::MessageDelta { stop_reason: finish, usage });
        yield Ok(StreamEvent::MessageStop);
    }
}
