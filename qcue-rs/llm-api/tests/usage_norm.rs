// QCue S1-R68, S1-R78 — fold the 4 vendor cache-token shapes + name variants into one CanonicalUsage.
#![allow(clippy::unwrap_used)]
use llm_api::usage_norm::{usage_from_anthropic, usage_from_chat};
use serde_json::json;

#[test]
fn test_chatcompletions_cached_tokens() {
    // OpenAI/chat: prompt_tokens_details.cached_tokens; prompt_tokens/completion_tokens names.
    let u = usage_from_chat(&json!({
        "prompt_tokens": 100, "completion_tokens": 40,
        "prompt_tokens_details": {"cached_tokens": 60},
        "completion_tokens_details": {"reasoning_tokens": 12}
    }))
    .unwrap();
    assert_eq!(u.input, 100);
    assert_eq!(u.output, 40);
    assert_eq!(u.cache_read, 60);
    assert_eq!(u.reasoning, 12);
}

#[test]
fn test_field_name_variants() {
    // S1-R78 — input_tokens/output_tokens also accepted.
    let u = usage_from_chat(&json!({"input_tokens": 7, "output_tokens": 3})).unwrap();
    assert_eq!((u.input, u.output), (7, 3));
}

#[test]
fn test_anthropic_cache_shape() {
    // Anthropic: cache_read_input_tokens / cache_creation_input_tokens.
    let u = usage_from_anthropic(&json!({
        "input_tokens": 200, "output_tokens": 50,
        "cache_read_input_tokens": 150, "cache_creation_input_tokens": 30
    }))
    .unwrap();
    assert_eq!(u.input, 200);
    assert_eq!(u.cache_read, 150);
    assert_eq!(u.cache_write, 30);
}
