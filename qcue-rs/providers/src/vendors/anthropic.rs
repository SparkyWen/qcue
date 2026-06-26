// QCue S1-R13 — Anthropic profile (M1 wired). cache_supported=true.
use crate::hooks::{Effort, ProviderHooks};
use crate::profile::{AuthType, ProviderProfile, TempPolicy};
use protocol::ApiMode;
use serde_json::Value;

/// Map QCue's effort levels onto Anthropic extended-thinking `budget_tokens`. Minimal disables
/// thinking entirely (returns None).
fn anthropic_budget_tokens(effort: Effort) -> Option<u64> {
    match effort {
        Effort::Minimal => None,
        Effort::Low => Some(2048),
        Effort::Medium => Some(4096),
        Effort::High => Some(8192),
        Effort::XHigh => Some(16384),
        Effort::Max => Some(32768),
    }
}

pub struct AnthropicHooks;

impl ProviderHooks for AnthropicHooks {
    fn apply_reasoning_effort(&self, body: &mut Value, effort: Effort) {
        let Some(budget) = anthropic_budget_tokens(effort) else {
            // Minimal → no extended thinking. Drop any pre-set thinking block.
            if let Some(obj) = body.as_object_mut() {
                obj.remove("thinking");
            }
            return;
        };
        body["thinking"] = serde_json::json!({"type": "enabled", "budget_tokens": budget});
        // Anthropic requires max_tokens > budget_tokens; only bump it when it's too small.
        let current = body.get("max_tokens").and_then(Value::as_u64).unwrap_or(0);
        if current <= budget {
            body["max_tokens"] = Value::from(budget + 4096);
        }
        // Extended thinking requires temperature = 1 (other values are rejected).
        body["temperature"] = Value::from(1);
    }
}

pub fn profile() -> ProviderProfile {
    let mut default_headers = std::collections::HashMap::new();
    default_headers.insert("anthropic-version".into(), "2023-06-01".into());
    let mut env_http_headers = std::collections::HashMap::new();
    env_http_headers.insert("x-api-key".into(), "ANTHROPIC_API_KEY".into());
    ProviderProfile {
        name: "anthropic".into(),
        api_mode: ApiMode::AnthropicMessages,
        base_url: "https://api.anthropic.com/v1".into(),
        models_url: None,
        auth_type: AuthType::ApiKey,
        default_headers,
        env_http_headers,
        fixed_temperature: TempPolicy::Inherit,
        default_max_tokens: Some(8192),
        fallback_models: vec![],
        supports_vision: true,
        request_max_retries: 3,
        stream_idle_timeout_ms: 30_000,
        stream_ttfb_timeout_ms: 90_000,
        cache_supported: true,
        hooks: Box::new(AnthropicHooks),
    }
}
