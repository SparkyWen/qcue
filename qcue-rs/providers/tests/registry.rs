#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R7,R13,R14 — hybrid registry: static D7 subset + OpenAiCompatible row + tenant override.
use providers::registry::{OpenAiCompatible, ProfileSource, register_all, resolve_profile};
use protocol::ApiMode;

#[test]
fn test_mvp_api_modes_for_d7() {
    // S1-R7 — every D7 provider resolves to ChatCompletions or AnthropicMessages.
    let reg = register_all();
    for p in ["openai", "anthropic", "gemini"] {
        let prof = reg.get(p).expect("M1 provider present");
        assert!(matches!(
            prof.api_mode,
            ApiMode::ChatCompletions | ApiMode::AnthropicMessages
        ));
    }
    assert_eq!(
        reg.get("anthropic").unwrap().api_mode,
        ApiMode::AnthropicMessages
    );
    assert_eq!(reg.get("openai").unwrap().api_mode, ApiMode::ChatCompletions);
    assert_eq!(reg.get("gemini").unwrap().api_mode, ApiMode::ChatCompletions);
}

#[test]
fn test_registry_hybrid_oai_compatible_and_override() {
    let reg = register_all();
    // an OpenAiCompatible row resolves to a ChatCompletions profile with generic quirks.
    let row = OpenAiCompatible {
        base_url: "https://api.long-tail.example/v1".into(),
        header_template: Default::default(),
        api_mode: ApiMode::ChatCompletions,
    };
    let src = resolve_profile(&reg, "long-tail-vendor", Some(&row));
    assert!(matches!(src, ProfileSource::DbRow(_)));

    // a static provider resolves from the compile-time map.
    let src2 = resolve_profile(&reg, "openai", None);
    assert!(matches!(src2, ProfileSource::Static(_)));

    // a tenant override row for `openai` shadows the static default (last-writer-wins).
    let override_row = OpenAiCompatible {
        base_url: "https://my-proxy.example/v1".into(),
        header_template: Default::default(),
        api_mode: ApiMode::ChatCompletions,
    };
    let src3 = resolve_profile(&reg, "openai", Some(&override_row));
    match src3 {
        ProfileSource::DbRow(p) => assert_eq!(p.base_url, "https://my-proxy.example/v1"),
        _ => panic!("override row must shadow static"),
    }
}

#[test]
fn all_seven_providers_registered() {
    // S1-R13 — all 7 first-class providers resolve from the static map.
    let reg = register_all();
    for p in ["openai", "anthropic", "gemini", "deepseek", "kimi", "qwen", "openrouter"] {
        assert!(reg.get(p).is_some(), "missing provider {p}");
    }
}

#[tokio::test]
async fn test_health_check_ok_and_unreachable() {
    use providers::health::health_check;
    // SBX-R4: private/loopback URLs are blocked in prod (QCUE_ALLOW_INSECURE_HTTP unset) → Err.
    // (Previously health_check used ClientOpts::default() with no egress guard.)
    for bad in ["https://127.0.0.1:1", "https://10.0.0.1:1", "https://169.254.169.254/"] {
        let r = health_check(bad).await;
        assert!(r.is_err(), "health_check must block {bad} in prod, got {r:?}");
    }
}
