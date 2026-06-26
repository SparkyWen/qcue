// QCue S1-R13 — OpenRouter aggregator (ChatCompletions). DefaultHooks; passthrough effort scaling.
use crate::hooks::DefaultHooks;
use crate::profile::{AuthType, ProviderProfile, TempPolicy};
use protocol::ApiMode;

pub fn profile() -> ProviderProfile {
    let mut env_http_headers = std::collections::HashMap::new();
    env_http_headers.insert("Authorization".into(), "OPENROUTER_API_KEY".into());
    ProviderProfile {
        name: "openrouter".into(),
        api_mode: ApiMode::ChatCompletions,
        base_url: "https://openrouter.ai/api/v1".into(),
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
        hooks: Box::new(DefaultHooks),
    }
}
