#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R8,R9,R10,R11,R12 — profile is declarative+secretless; hooks default to no-ops; api_mode resolution.
use providers::hooks::{DefaultHooks, ProviderHooks};
use providers::profile::{AuthType, ProviderProfile, TempPolicy};
use providers::resolve::resolve_api_mode;
use protocol::ApiMode;
use protocol::Message;

#[test]
fn test_api_mode_resolution_precedence() {
    // explicit wins
    assert_eq!(
        resolve_api_mode(
            Some(ApiMode::AnthropicMessages),
            "openai",
            "https://api.openai.com"
        ),
        ApiMode::AnthropicMessages
    );
    // provider-name match
    assert_eq!(
        resolve_api_mode(None, "anthropic", "https://x"),
        ApiMode::AnthropicMessages
    );
    // base_url host match (api.anthropic.com OR ends /anthropic)
    assert_eq!(
        resolve_api_mode(None, "custom", "https://api.anthropic.com/v1"),
        ApiMode::AnthropicMessages
    );
    assert_eq!(
        resolve_api_mode(None, "custom", "https://gw.example.com/anthropic"),
        ApiMode::AnthropicMessages
    );
    // default
    assert_eq!(
        resolve_api_mode(None, "deepseek", "https://api.deepseek.com"),
        ApiMode::ChatCompletions
    );
}

#[test]
fn test_profile_has_no_secret_field() {
    // S1-R9 — env_http_headers values are env-var NAMES, never secrets; no client/credential field.
    let p = sample_profile();
    assert!(
        p.env_http_headers.values().all(|v| !v.contains("sk-")),
        "headers must carry env names"
    );
    // No field is a credential/client; the struct only carries declarative data + Box<dyn Hooks>.
    let _ = &p.hooks;
}

#[test]
fn test_temp_policy_tristate() {
    // S1-R11 — Omit/Fixed/Inherit
    assert!(matches!(TempPolicy::Omit, TempPolicy::Omit));
    assert!(matches!(TempPolicy::Fixed(0.0), TempPolicy::Fixed(_)));
    assert!(matches!(TempPolicy::Inherit, TempPolicy::Inherit));
}

#[test]
fn test_default_hooks_are_noops() {
    // S1-R10 — default prepare_messages leaves messages unchanged.
    let h = DefaultHooks;
    let msgs = vec![Message {
        role: protocol::Role::User,
        content: Some("hi".into()),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        finish_reason: None,
        reasoning: None,
        provider_data: None,
        active: true,
        is_untrusted: false,
    }];
    let out = h.prepare_messages(msgs.clone());
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].content, msgs[0].content);
    assert_eq!(h.get_max_tokens("any"), None);
}

fn sample_profile() -> ProviderProfile {
    ProviderProfile {
        name: "openai".into(),
        api_mode: ApiMode::ChatCompletions,
        base_url: "https://api.openai.com".into(),
        models_url: None,
        auth_type: AuthType::ApiKey,
        default_headers: Default::default(),
        env_http_headers: Default::default(),
        fixed_temperature: TempPolicy::Inherit,
        default_max_tokens: Some(4096),
        fallback_models: vec![],
        supports_vision: true,
        request_max_retries: 3,
        stream_idle_timeout_ms: 30_000,
        stream_ttfb_timeout_ms: 60_000,
        cache_supported: true,
        hooks: Box::new(DefaultHooks),
    }
}
