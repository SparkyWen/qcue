// QCue S1-R62..R69 — prefix byte-stability, cache hints (gated, deep-copied), usage dedup by request_id.
use protocol::CanonicalUsage;
use serde_json::{json, Value};
use std::collections::HashMap;

/// S1-R62/R63 — assemble the system prompt from stable parts in fixed order; no volatile bytes.
pub fn build_system_prompt(parts: &[String]) -> String {
    parts.join("\n\n")
}

/// djb2 hash over the serialized bytes (S1-R65 cache-break attribution primitive).
pub fn hash_bytes(s: &str) -> u64 {
    let mut h: u64 = 5381;
    for b in s.as_bytes() {
        h = h.wrapping_mul(33).wrapping_add(*b as u64);
    }
    h
}

/// S1-R66/R67 — mark Anthropic prompt-cache breakpoints on an ALREADY-BUILT request body, IN PLACE.
/// Critical wire rule: `cache_control` is legal only on a content BLOCK (or a system text block) —
/// NEVER as a top-level field of the message object (that is a 400 "messages.N.cache_control: Extra
/// inputs are not permitted"). We breakpoint the system prefix and the last message's last content
/// block (≤4 breakpoints, the public system+last policy). Never emits the non-portable cache_reference.
///
/// The body is what the AnthropicTransport already produced: `system` is a string (hoisted) and each
/// message carries `content` as an array of blocks. We must NOT rebuild messages here (that would
/// drop tool_use/thinking blocks) — we only annotate.
pub fn apply_anthropic_cache_control_to_body(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else { return };

    // system: a plain string → a single cacheable text block; an existing array → mark its last block.
    let sys_text = match obj.get("system") {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    if let Some(text) = sys_text {
        obj.insert(
            "system".into(),
            json!([{ "type": "text", "text": text, "cache_control": {"type": "ephemeral"} }]),
        );
    } else if let Some(Value::Array(blocks)) = obj.get_mut("system")
        && let Some(last) = blocks.last_mut()
    {
        mark_block_cache_control(last);
    }

    // messages: mark the last content block of the last message that has a non-empty block array.
    if let Some(Value::Array(msgs)) = obj.get_mut("messages") {
        for m in msgs.iter_mut().rev() {
            if let Some(Value::Array(blocks)) = m.get_mut("content")
                && let Some(last) = blocks.last_mut()
            {
                mark_block_cache_control(last);
                break;
            }
        }
    }
}

/// Add `cache_control: {type: ephemeral}` to a single content-block object (no-op for non-objects).
fn mark_block_cache_control(block: &mut Value) {
    if let Some(obj) = block.as_object_mut() {
        obj.insert("cache_control".into(), json!({"type":"ephemeral"}));
    }
}

/// A usage record as it appears per content-block (the whole-call value repeats; S1-R69).
#[derive(Clone, Debug)]
pub struct UsageRecord {
    pub request_id: Option<String>,
    pub usage: CanonicalUsage,
}

/// S1-R69 — dedup by request_id (take last per id), then sum across distinct requests.
pub fn dedup_usage_by_request_id(records: &[UsageRecord]) -> CanonicalUsage {
    let mut by_id: HashMap<String, CanonicalUsage> = HashMap::new();
    let mut anon = CanonicalUsage::default();
    for r in records {
        match &r.request_id {
            Some(id) => {
                by_id.insert(id.clone(), r.usage); // last wins
            }
            None => {
                anon.input += r.usage.input;
                anon.output += r.usage.output;
                anon.cache_read += r.usage.cache_read;
                anon.cache_write += r.usage.cache_write;
                anon.reasoning += r.usage.reasoning;
            }
        }
    }
    let mut total = anon;
    for u in by_id.values() {
        total.input += u.input;
        total.output += u.output;
        total.cache_read += u.cache_read;
        total.cache_write += u.cache_write;
        total.reasoning += u.reasoning;
    }
    total
}
