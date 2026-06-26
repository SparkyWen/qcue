// QCue S1-R70 — minimal compressor stub. The 116KB trajectory compressor is M6+.
use protocol::{Message, Role};
use std::future::Future;

/// S1-R70 — trigger when the preflight estimate exceeds context_window * 0.8.
pub fn needs_compression(estimated_tokens: u64, context_window: u64) -> bool {
    estimated_tokens as f64 > context_window as f64 * 0.8
}

/// Summarize the oldest `count` non-system turns into one summary message; clean orphan tool_results.
pub async fn compress_oldest<F, Fut>(history: &mut Vec<Message>, count: usize, summarize: F)
where
    F: FnOnce(Vec<Message>) -> Fut,
    Fut: Future<Output = String>,
{
    let sys: Vec<Message> = history.iter().filter(|m| m.role == Role::System).cloned().collect();
    let non_sys: Vec<Message> = history.iter().filter(|m| m.role != Role::System).cloned().collect();
    if non_sys.len() <= count {
        return;
    }
    let (old, recent) = non_sys.split_at(count);
    let summary_text = summarize(old.to_vec()).await;
    let summary = Message {
        role: Role::Assistant,
        content: Some(summary_text),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        finish_reason: None,
        reasoning: None,
        provider_data: None,
        active: true,
        is_untrusted: false,
    };
    let mut rebuilt = sys;
    rebuilt.push(summary);
    rebuilt.extend(recent.iter().cloned());
    // clean orphan tool_results (a Role::Tool with no preceding assistant tool_calls).
    rebuilt = clean_orphan_tool_results(rebuilt);
    *history = rebuilt;
}

/// Drop a Role::Tool message whose tool_call_id has no matching tool_call in a preceding assistant msg.
pub fn clean_orphan_tool_results(msgs: Vec<Message>) -> Vec<Message> {
    let mut known_ids: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for m in msgs {
        if let Some(tcs) = &m.tool_calls {
            for tc in tcs {
                if let Some(id) = &tc.id {
                    known_ids.push(id.clone());
                }
            }
        }
        if m.role == Role::Tool {
            match &m.tool_call_id {
                Some(id) if known_ids.contains(id) => out.push(m),
                _ => {} // orphan → drop
            }
        } else {
            out.push(m);
        }
    }
    out
}
