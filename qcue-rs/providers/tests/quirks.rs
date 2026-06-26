#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R71..R78 — every quirk is tested data (S1-R3).
use providers::hooks::Effort;
use providers::quirks::{
    ThinkingDisable, is_reasoning_provider, needs_reasoning_replay, scale_effort,
    thinking_disable_shape,
};
use serde_json::json;

#[test]
fn test_q1_q3_reasoning_replay_decision_tuple() {
    // S1-R71/R73 — (provider, model, effort) tuple-aware.
    // DeepSeek model on a generic openai-typed provider STILL needs reasoning_content replay.
    assert!(needs_reasoning_replay("openrouter", "deepseek-reasoner"));
    assert!(needs_reasoning_replay("deepseek", "deepseek-chat"));
    // A non-DeepSeek model on a non-accept-list provider does NOT.
    assert!(!needs_reasoning_replay("openai", "gpt-4o"));
}

#[test]
fn test_q4_thinking_disable_shapes() {
    // S1-R74 — provider-specific disable shapes.
    assert_eq!(
        thinking_disable_shape("deepseek"),
        ThinkingDisable::ThinkingTypeDisabled
    );
    assert_eq!(
        thinking_disable_shape("siliconflow"),
        ThinkingDisable::ThinkingTypeDisabled
    );
    assert_eq!(
        thinking_disable_shape("vllm"),
        ThinkingDisable::ChatTemplateEnableThinkingFalse
    );
    assert_eq!(
        thinking_disable_shape("nvidia"),
        ThinkingDisable::ChatTemplateThinkingFalse
    );
    assert_eq!(thinking_disable_shape("openai"), ThinkingDisable::Ignored);
    assert_eq!(thinking_disable_shape("moonshot"), ThinkingDisable::Ignored);
}

#[test]
fn test_q5_effort_scaling() {
    // S1-R74 — DeepSeek low/medium→high, xhigh→max; vLLM downgrades max→high; OpenRouter passthrough.
    assert_eq!(scale_effort("deepseek", Effort::Low), Effort::High);
    assert_eq!(scale_effort("deepseek", Effort::Medium), Effort::High);
    assert_eq!(scale_effort("deepseek", Effort::XHigh), Effort::Max);
    assert_eq!(scale_effort("vllm", Effort::Max), Effort::High);
    assert_eq!(scale_effort("openrouter", Effort::Medium), Effort::Medium);
}

#[test]
fn test_q7_answer_in_reasoning_fallback() {
    // S1-R76 — a non-reasoning provider streaming an answer in reasoning_content renders it as text.
    assert!(!is_reasoning_provider("openai"));
    assert!(is_reasoning_provider("deepseek"));
}

#[test]
fn test_deepseek_apply_reasoning_effort_body() {
    // S1-R74 — the DeepSeek hook emits {"thinking":{"type":"disabled"}} when effort is Minimal.
    use providers::hooks::ProviderHooks;
    use providers::vendors::deepseek::DeepSeekHooks;
    let h = DeepSeekHooks;
    let mut body = json!({});
    h.apply_reasoning_effort(&mut body, Effort::Minimal);
    assert_eq!(body["thinking"]["type"], "disabled");
    let mut body2 = json!({});
    h.apply_reasoning_effort(&mut body2, Effort::Low);
    // Low scales to High → reasoning_effort present, no disable.
    assert_eq!(body2["reasoning_effort"], "high");
}

#[test]
fn test_openai_apply_reasoning_effort_body() {
    // OpenAI reasoning_effort wire rules. THE load-bearing constraint (see
    // docs/postmortems/2026-06-19-gpt5x-reasoning-effort-breaks-tools.md): the gpt-5.x **dot-minor**
    // generation (gpt-5.1 / 5.4 / 5.5 …) REJECTS `reasoning_effort` on /v1/chat/completions. With
    // function tools present OpenAI returns
    //   400 "Function tools with reasoning_effort are not supported for gpt-5.5 in /v1/chat/completions"
    // (it advises /v1/responses). QCue is chat-completions-only for OpenAI (Responses is M6+/NG1) and
    // every recall/Dream turn advertises function tools, so emitting reasoning_effort for that
    // generation 400s the turn → tool calling + web (联网) silently break. So gpt-5.x must get NOTHING.
    use providers::hooks::ProviderHooks;
    use providers::vendors::openai::OpenAiHooks;
    let h = OpenAiHooks;

    // gpt-5.x dot-minor (incl. the curated catalog ids gpt-5.5 / gpt-5.4-mini) → reasoning_effort
    // OMITTED for EVERY effort level (any value would 400 alongside function tools).
    for model in ["gpt-5.5", "gpt-5.4-mini", "gpt-5.1", "gpt-5.4"] {
        for effort in [Effort::Minimal, Effort::Low, Effort::Medium, Effort::High, Effort::Max] {
            let mut body = json!({ "model": model });
            h.apply_reasoning_effort(&mut body, effort);
            assert!(
                body.get("reasoning_effort").is_none(),
                "{model} is gpt-5.x → reasoning_effort must be OMITTED on chat/completions \
                 (it 400s with function tools; use /v1/responses); effort={effort:?}, body={body}",
            );
        }
    }

    // The ORIGINAL gpt-5 generation accepts reasoning_effort on chat completions (incl. "minimal").
    let mut body = json!({"model": "gpt-5"});
    h.apply_reasoning_effort(&mut body, Effort::High);
    assert_eq!(body["reasoning_effort"], "high");

    let mut body = json!({"model": "gpt-5"});
    h.apply_reasoning_effort(&mut body, Effort::Max);
    assert_eq!(body["reasoning_effort"], "high", "base gpt-5 chat tops out at high → clamp");

    let mut body = json!({"model": "gpt-5-mini"});
    h.apply_reasoning_effort(&mut body, Effort::Minimal);
    assert_eq!(body["reasoning_effort"], "minimal", "base gpt-5 supports minimal on chat completions");

    // o-series accepts reasoning_effort on chat completions; no "minimal" tier → Minimal clamps to "low".
    let mut body = json!({"model": "o3"});
    h.apply_reasoning_effort(&mut body, Effort::Minimal);
    assert_eq!(body["reasoning_effort"], "low");

    let mut body = json!({"model": "o4-mini"});
    h.apply_reasoning_effort(&mut body, Effort::High);
    assert_eq!(body["reasoning_effort"], "high");

    // a non-reasoning chat model must NOT get the key (it would 400).
    let mut body = json!({"model": "gpt-4.5"});
    h.apply_reasoning_effort(&mut body, Effort::High);
    assert!(body.get("reasoning_effort").is_none(), "non-reasoning model: no reasoning_effort");
}

#[test]
fn test_anthropic_apply_reasoning_effort_body() {
    // Anthropic: effort → extended-thinking budget_tokens, with max_tokens > budget and temp=1.
    use providers::hooks::ProviderHooks;
    use providers::vendors::anthropic::AnthropicHooks;
    let h = AnthropicHooks;

    let mut body = json!({"model": "claude-opus-4-8", "max_tokens": 1024});
    h.apply_reasoning_effort(&mut body, Effort::Medium);
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(body["thinking"]["budget_tokens"], 4096);
    assert!(
        body["max_tokens"].as_u64().unwrap() > 4096,
        "max_tokens must exceed budget_tokens"
    );
    assert_eq!(body["temperature"], 1, "extended thinking requires temperature=1");

    // a larger pre-existing max_tokens is preserved (only bumped when too small).
    let mut body = json!({"model": "claude-opus-4-8", "max_tokens": 64000});
    h.apply_reasoning_effort(&mut body, Effort::Max);
    assert_eq!(body["thinking"]["budget_tokens"], 32768);
    assert_eq!(body["max_tokens"], 64000);

    // Minimal disables extended thinking (no thinking block).
    let mut body = json!({"model": "claude-opus-4-8", "max_tokens": 1024});
    h.apply_reasoning_effort(&mut body, Effort::Minimal);
    assert!(body.get("thinking").is_none(), "Minimal → no extended thinking");
}
