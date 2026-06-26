// QCue S1-R74 — DeepSeek reasoning-quirk hooks (coded at M1; provider WIRED only at M6+).
use crate::hooks::{Effort, ProviderHooks};
use crate::profile::{AuthType, ProviderProfile, TempPolicy};
use crate::quirks::{effort_str, scale_effort};
use protocol::ApiMode;
use serde_json::Value;

/// S1-R13 — DeepSeek profile (ChatCompletions; reasoning replay via DeepSeekHooks). No prompt cache.
pub fn profile() -> ProviderProfile {
    let mut env_http_headers = std::collections::HashMap::new();
    env_http_headers.insert("Authorization".into(), "DEEPSEEK_API_KEY".into());
    ProviderProfile {
        name: "deepseek".into(),
        api_mode: ApiMode::ChatCompletions,
        base_url: "https://api.deepseek.com".into(),
        models_url: None,
        auth_type: AuthType::ApiKey,
        default_headers: Default::default(),
        env_http_headers,
        fixed_temperature: TempPolicy::Inherit,
        default_max_tokens: Some(4096),
        fallback_models: vec![],
        supports_vision: false,
        request_max_retries: 3,
        stream_idle_timeout_ms: 30_000,
        stream_ttfb_timeout_ms: 60_000,
        cache_supported: false,
        hooks: Box::new(DeepSeekHooks),
    }
}

pub struct DeepSeekHooks;

impl ProviderHooks for DeepSeekHooks {
    fn apply_reasoning_effort(&self, body: &mut Value, effort: Effort) {
        if matches!(effort, Effort::Minimal) {
            body["thinking"] = serde_json::json!({"type": "disabled"});
            return;
        }
        let scaled = scale_effort("deepseek", effort);
        body["reasoning_effort"] = Value::String(effort_str(scaled).to_string());
    }
}
