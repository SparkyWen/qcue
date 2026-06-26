#![allow(clippy::unwrap_used, clippy::expect_used, clippy::needless_borrows_for_generic_args)]
// QCue S1-R7,R17,R18,R20,R25,R30,R39,R66,R88 — protocol type contracts
use protocol::*;
use serde_json::json;

#[test]
fn test_mvp_api_modes_serde() {
    // S1-R7 — only ChatCompletions + AnthropicMessages are MVP; both round-trip.
    assert_eq!(serde_json::to_string(&ApiMode::ChatCompletions).unwrap(), "\"ChatCompletions\"");
    assert_eq!(serde_json::to_string(&ApiMode::AnthropicMessages).unwrap(), "\"AnthropicMessages\"");
}

#[test]
fn test_normalized_surface_minimal() {
    // S1-R17 — exactly six fields; vendor state only via provider_data.
    let nr = NormalizedResponse {
        content: Some("hi".into()), tool_calls: None,
        finish_reason: FinishReason::Stop, reasoning: None,
        usage: None, provider_data: None,
    };
    assert_eq!(nr.content.as_deref(), Some("hi"));
}

#[test]
fn test_tool_args_is_json_string() {
    // S1-R18 — ToolCall.arguments is a raw JSON STRING, byte-stable.
    let tc = ToolCall { id: Some("call_0".into()), name: "recall_search".into(),
        arguments: "{\"pattern\":\"x\"}".into(), provider_data: None };
    let s = serde_json::to_string(&tc).unwrap();
    let back: ToolCall = serde_json::from_str(&s).unwrap();
    assert_eq!(back.arguments, "{\"pattern\":\"x\"}");
}

#[test]
fn test_superset_one_bag() {
    // S1-R25 — exactly one opaque provider_data bag; no vendor-native top-level fields.
    let m = Message { role: Role::Assistant, content: None, tool_call_id: None,
        tool_name: None, tool_calls: None, finish_reason: None, reasoning: None,
        provider_data: Some(json!({"reasoning_content": "deepseek", "signature": "anthropic"})),
        active: true, is_untrusted: false };
    let bag = m.provider_data.unwrap();
    assert!(bag.get("reasoning_content").is_some() && bag.get("signature").is_some());
}

#[test]
fn test_canonical_usage_five_fields() {
    // S1-R66 — 5-field struct: input/output/cache_read/cache_write/reasoning.
    let u = CanonicalUsage { input: 1, output: 2, cache_read: 3, cache_write: 4, reasoning: 5 };
    assert_eq!(u.reasoning, 5);
    let v = serde_json::to_value(&u).unwrap();
    for k in ["input","output","cache_read","cache_write","reasoning"] {
        assert!(v.get(k).is_some(), "usage missing {k}");
    }
}

#[test]
fn test_cred_status_carries_cooldown() {
    // S1-R30 — Exhausted carries `until` in the type.
    let s = CredStatus::Exhausted { until_ms: 12345 };
    matches!(s, CredStatus::Exhausted { until_ms } if until_ms == 12345);
}

#[test]
fn test_stream_event_taxonomy() {
    // S1-R20 — the 6 internal event kinds + Block/Delta.
    let _ = StreamEvent::MessageStart;
    let _ = StreamEvent::ContentBlockStart(Block::ToolUse { id: "1".into(), name: "t".into() });
    let _ = StreamEvent::ContentBlockDelta(Delta::TextDelta("a".into()));
    let _ = StreamEvent::ContentBlockStop;
    let _ = StreamEvent::MessageDelta { stop_reason: Some(FinishReason::Stop), usage: None };
    let _ = StreamEvent::MessageStop;
    let _ = Delta::ThinkingDelta("t".into());
    let _ = Delta::InputJsonDelta { partial_json: "{".into() };
    let _ = Block::Text;
    let _ = Block::Thinking;
}

#[test]
fn test_appendix_a_helpers() {
    // S1-R2 — WikiEditOp / DreamPhase / Citation defined in protocol for Dart codegen.
    let c = Citation { rel_path: "wiki/entities/x.md".into(), start_line: 10, end_line: 20 };
    assert_eq!(c.start_line, 10);
    let _ = (WikiEditOp::Create, WikiEditOp::Update, WikiEditOp::Merge { into_slug: "x".into() }, WikiEditOp::Delete);
    // Merge carries the target slug; the serde tag="type" wire keeps it forward-compatible with S3.
    let merge = serde_json::to_value(WikiEditOp::Merge { into_slug: "y".into() }).unwrap();
    assert_eq!(merge["type"], "Merge");
    assert_eq!(merge["into_slug"], "y");
    let _ = (DreamPhase::Orient, DreamPhase::Gather, DreamPhase::Consolidate, DreamPhase::Prune);
}

#[test]
fn test_classified_error_four_bits() {
    // S1-R39 — ClassifiedError carries 4 action bits + reason + reset.
    let ce = ClassifiedError { reason: FailoverReason::RateLimit, status_code: Some(429),
        retryable: true, should_compress: false, should_rotate_credential: true,
        should_fallback: false, reset_at_ms: Some(30_000) };
    assert!(ce.should_rotate_credential && !ce.should_fallback);
}

#[test]
fn test_runtime_event_envelope_forward_compat() {
    // S1-R88 — unknown event kinds survive deserialization (payload stays opaque).
    let raw = json!({"schema_version": 1, "event": "futureKind", "payload": {"x": 1}});
    let env: RuntimeEventEnvelope = serde_json::from_value(raw).unwrap();
    assert_eq!(env.event, "futureKind");
    assert_eq!(env.schema_version, 1);
}
