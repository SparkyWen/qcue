// QCue S1-R68, S1-R78 — usage normalization for billing. One CanonicalUsage, N vendor shapes.
use protocol::CanonicalUsage;
use serde_json::Value;

fn u64_at(v: &Value, keys: &[&str]) -> u64 {
    for k in keys {
        if let Some(n) = v.get(*k).and_then(|x| x.as_u64()) {
            return n;
        }
    }
    0
}

/// ChatCompletions + Responses-fallback shapes (S1-R68 #2/#3/#4, S1-R78 names).
pub fn usage_from_chat(v: &Value) -> Option<CanonicalUsage> {
    if v.is_null() || !v.is_object() {
        return None;
    }
    let input = u64_at(v, &["prompt_tokens", "input_tokens"]);
    let output = u64_at(v, &["completion_tokens", "output_tokens"]);
    let cache_read = v
        .get("prompt_tokens_details")
        .map(|d| u64_at(d, &["cached_tokens"]))
        .or_else(|| v.get("input_tokens_details").map(|d| u64_at(d, &["cached_tokens"])))
        .unwrap_or_else(|| u64_at(v, &["cached_tokens"]));
    // Chat nests reasoning under `completion_tokens_details`; the Responses API nests it under
    // `output_tokens_details` (RESP-R7). Check both so a Responses turn doesn't silently read 0
    // reasoning tokens and under-bill the cost ledger.
    let reasoning = v
        .get("completion_tokens_details")
        .map(|d| u64_at(d, &["reasoning_tokens"]))
        .filter(|n| *n > 0)
        .or_else(|| v.get("output_tokens_details").map(|d| u64_at(d, &["reasoning_tokens"])))
        .filter(|n| *n > 0)
        .unwrap_or_else(|| u64_at(v, &["reasoning_tokens"]));
    Some(CanonicalUsage { input, output, cache_read, cache_write: 0, reasoning })
}

/// Anthropic shape (S1-R68 #1): cache_read_input_tokens / cache_creation_input_tokens.
pub fn usage_from_anthropic(v: &Value) -> Option<CanonicalUsage> {
    if v.is_null() || !v.is_object() {
        return None;
    }
    Some(CanonicalUsage {
        input: u64_at(v, &["input_tokens"]),
        output: u64_at(v, &["output_tokens"]),
        cache_read: u64_at(v, &["cache_read_input_tokens"]),
        cache_write: u64_at(v, &["cache_creation_input_tokens"]),
        reasoning: 0,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_reasoning_from_output_tokens_details() {
        // RESP-R7 — the Responses API nests reasoning tokens under output_tokens_details, not
        // completion_tokens_details. Without this the cost ledger under-bills reasoning to 0.
        let v = json!({
            "input_tokens": 10, "output_tokens": 20,
            "input_tokens_details": { "cached_tokens": 3 },
            "output_tokens_details": { "reasoning_tokens": 7 }
        });
        let u = usage_from_chat(&v).unwrap();
        assert_eq!(u.input, 10);
        assert_eq!(u.output, 20);
        assert_eq!(u.cache_read, 3);
        assert_eq!(u.reasoning, 7, "Responses reasoning nests under output_tokens_details");
    }

    #[test]
    fn still_reads_chat_completion_tokens_details() {
        // chat/completions path must keep working (reasoning under completion_tokens_details).
        let v = json!({
            "prompt_tokens": 5, "completion_tokens": 9,
            "completion_tokens_details": { "reasoning_tokens": 4 }
        });
        let u = usage_from_chat(&v).unwrap();
        assert_eq!(u.input, 5);
        assert_eq!(u.output, 9);
        assert_eq!(u.reasoning, 4);
    }
}
