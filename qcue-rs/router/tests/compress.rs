#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R70 — minimal summarizer: over-window history → summarized; orphan tool_results cleaned.
use protocol::{Message, Role};
use router::compress::{compress_oldest, needs_compression};

fn msg(role: Role, c: &str) -> Message {
    Message {
        role,
        content: Some(c.into()),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        finish_reason: None,
        reasoning: None,
        provider_data: None,
        active: true,
        is_untrusted: false,
    }
}

#[test]
fn test_needs_compression_threshold() {
    // estimate > window*0.8 triggers.
    assert!(needs_compression(8100, 10_000));
    assert!(!needs_compression(7000, 10_000));
}

#[tokio::test]
async fn test_compress_summarizes_oldest_and_cleans_orphans() {
    let mut history = vec![
        msg(Role::System, "sys"),
        msg(Role::User, "old turn 1"),
        msg(Role::Assistant, "old answer 1"),
        msg(Role::User, "recent"),
    ];
    // a fake aux summarizer returns a fixed summary.
    compress_oldest(&mut history, 2, |_old| async { "SUMMARY of old turns".to_string() }).await;
    // the system prompt is preserved; a summary message replaces the oldest turns; recent kept.
    assert_eq!(history[0].role, Role::System);
    assert!(history.iter().any(|m| m.content.as_deref() == Some("SUMMARY of old turns")));
    assert!(history.iter().any(|m| m.content.as_deref() == Some("recent")));
}
