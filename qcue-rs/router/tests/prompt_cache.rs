#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R62..R69 — prefix byte-stability, cache hints gated+deepcopied, usage dedup by request_id.
use protocol::CanonicalUsage;
use router::prompt_cache::{
    apply_anthropic_cache_control_to_body, build_system_prompt, dedup_usage_by_request_id,
    hash_bytes, UsageRecord,
};
use serde_json::json;

#[test]
fn test_system_prompt_byte_stable() {
    // S1-R62/R63 — building twice from the same parts is byte-identical (hash-equal).
    let parts = vec!["You are QCue.".to_string(), "Rules: ...".to_string()];
    let a = build_system_prompt(&parts);
    let b = build_system_prompt(&parts);
    assert_eq!(hash_bytes(&a), hash_bytes(&b));
    assert_eq!(a, b);
}

#[test]
fn test_cache_control_lands_on_content_blocks_not_message_object() {
    // S1-R66 — cache_control is legal ONLY on a content block / system text block. A top-level
    // `cache_control` on the message object is a hard 400 from Anthropic. The transport hands us a
    // body with `system` as a string and each message's `content` as a block array.
    let mut body = json!({
        "model": "claude-haiku-4-5",
        "system": "You are QCue.",
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "u1"}]},
            {"role": "assistant", "content": [
                {"type": "text", "text": "a1"},
                {"type": "tool_use", "id": "t1", "name": "recall_search", "input": {"q": "x"}}
            ]},
        ],
    });
    apply_anthropic_cache_control_to_body(&mut body);

    // system: a plain string becomes a single cacheable text block.
    let sys = body["system"].as_array().expect("system hoisted to a block array");
    assert_eq!(sys[0]["type"], "text");
    assert_eq!(sys[0]["text"], "You are QCue.");
    assert_eq!(sys[0]["cache_control"]["type"], "ephemeral");

    // messages: NO message object carries a top-level cache_control.
    let msgs = body["messages"].as_array().unwrap();
    assert!(
        msgs.iter().all(|m| m.get("cache_control").is_none()),
        "cache_control must never be a top-level message field: {body}"
    );
    // the LAST block of the LAST message is the breakpoint.
    let last_blocks = msgs.last().unwrap()["content"].as_array().unwrap();
    assert!(last_blocks.last().unwrap().get("cache_control").is_some());
    // earlier blocks are NOT marked (one breakpoint per message).
    assert!(last_blocks[0].get("cache_control").is_none());

    // ≤4 breakpoints total (here: system + last message = 2).
    let n = serde_json::to_string(&body).unwrap().matches("cache_control").count();
    assert!((1..=4).contains(&n), "expected 1..=4 breakpoints, got {n}");
}

#[test]
fn test_no_cache_reference_emitted() {
    // S1-R67 — the non-portable cache_reference extension never appears.
    let mut body = json!({
        "system": "s",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
    });
    apply_anthropic_cache_control_to_body(&mut body);
    assert!(!serde_json::to_string(&body).unwrap().contains("cache_reference"));
}

#[test]
fn test_usage_dedup_by_request_id() {
    // S1-R69 — usage repeated across N records for one request contributes once.
    let recs = vec![
        UsageRecord {
            request_id: Some("r1".into()),
            usage: CanonicalUsage { input: 100, output: 20, ..Default::default() },
        },
        UsageRecord {
            request_id: Some("r1".into()),
            usage: CanonicalUsage { input: 100, output: 20, ..Default::default() },
        },
        UsageRecord {
            request_id: Some("r2".into()),
            usage: CanonicalUsage { input: 50, output: 10, ..Default::default() },
        },
    ];
    let total = dedup_usage_by_request_id(&recs);
    assert_eq!(total.input, 150); // r1 once + r2 once, not 250
    assert_eq!(total.output, 30);
}
