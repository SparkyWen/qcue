// QCue S1-R13,R72 — Kimi / Moonshot profile (ChatCompletions). Reasoning is `reasoning_effort`
// XOR `thinking` (never both): Minimal disables thinking; any other effort sets reasoning_effort only.
use crate::hooks::{Effort, ProviderHooks};
use crate::profile::{AuthType, ProviderProfile, TempPolicy};
use crate::quirks::{effort_str, scale_effort};
use protocol::ApiMode;
use serde_json::Value;

pub fn profile() -> ProviderProfile {
    let mut env_http_headers = std::collections::HashMap::new();
    env_http_headers.insert("Authorization".into(), "MOONSHOT_API_KEY".into());
    ProviderProfile {
        name: "kimi".into(),
        api_mode: ApiMode::ChatCompletions,
        base_url: "https://api.moonshot.cn/v1".into(),
        models_url: None,
        auth_type: AuthType::ApiKey,
        default_headers: Default::default(),
        env_http_headers,
        // Moonshot ignores/rejects temperature for some thinking models → omit the key entirely.
        fixed_temperature: TempPolicy::Omit,
        default_max_tokens: Some(4096),
        fallback_models: vec![],
        supports_vision: false,
        request_max_retries: 3,
        stream_idle_timeout_ms: 30_000,
        stream_ttfb_timeout_ms: 60_000,
        cache_supported: false,
        hooks: Box::new(KimiHooks),
    }
}

pub struct KimiHooks;

impl ProviderHooks for KimiHooks {
    fn apply_reasoning_effort(&self, body: &mut Value, effort: Effort) {
        // XOR: Minimal disables thinking and never emits reasoning_effort.
        if matches!(effort, Effort::Minimal) {
            body["thinking"] = serde_json::json!({"type": "disabled"});
            return;
        }
        // Any other effort sets reasoning_effort ONLY (never also `thinking`).
        let scaled = scale_effort("kimi", effort);
        body["reasoning_effort"] = Value::String(effort_str(scaled).to_string());
    }
}
