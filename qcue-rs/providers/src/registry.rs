// QCue S1-R13 — hybrid registry: static D7 map (M1 subset wired) + OpenAiCompatible DB rows.
use crate::hooks::DefaultHooks;
use crate::profile::{AuthType, ProviderProfile, TempPolicy};
use crate::vendors::{anthropic, deepseek, gemini, kimi, openai, openrouter, qwen};
use protocol::ApiMode;
use std::collections::HashMap;

pub struct Registry {
    profiles: HashMap<String, ProviderProfile>,
}
impl Registry {
    pub fn get(&self, name: &str) -> Option<&ProviderProfile> {
        self.profiles.get(name)
    }

    /// Build a registry from an explicit profile map (used to point a provider's `base_url` at a
    /// test/mock server, or to compose a tenant-specific subset).
    pub fn from_profiles(profiles: HashMap<String, ProviderProfile>) -> Self {
        Self { profiles }
    }

    /// Insert/override a single profile (last-writer-wins).
    pub fn insert(&mut self, name: impl Into<String>, profile: ProviderProfile) {
        self.profiles.insert(name.into(), profile);
    }
}

/// Build the static compile-time registry. All 7 first-class D7 providers are wired (hooks coded).
pub fn register_all() -> Registry {
    let mut profiles = HashMap::new();
    profiles.insert("openai".into(), openai::profile());
    profiles.insert("anthropic".into(), anthropic::profile());
    profiles.insert("gemini".into(), gemini::profile());
    profiles.insert("deepseek".into(), deepseek::profile());
    profiles.insert("kimi".into(), kimi::profile());
    profiles.insert("qwen".into(), qwen::profile());
    profiles.insert("openrouter".into(), openrouter::profile());
    Registry { profiles }
}

/// A tenant-registered long-tail vendor (DB row), or a tenant override for a first-class provider.
#[derive(Clone, Debug)]
pub struct OpenAiCompatible {
    pub base_url: String,
    pub header_template: HashMap<String, String>,
    pub api_mode: ApiMode,
}

pub enum ProfileSource<'a> {
    Static(&'a ProviderProfile),
    // Boxed: a `ProviderProfile` is ~248 bytes vs the 8-byte `Static` reference; boxing the
    // large owned variant satisfies clippy's `large_enum_variant` (toolchain 1.96). The test
    // accesses `p.base_url` via auto-deref, so the API is unchanged.
    DbRow(Box<ProviderProfile>),
}

/// last-writer-wins: a tenant DB row (override or long-tail) shadows the static default.
pub fn resolve_profile<'a>(
    reg: &'a Registry,
    provider: &str,
    row: Option<&OpenAiCompatible>,
) -> ProfileSource<'a> {
    if let Some(r) = row {
        return ProfileSource::DbRow(Box::new(ProviderProfile {
            name: provider.to_string(),
            api_mode: r.api_mode,
            base_url: r.base_url.clone(),
            models_url: None,
            auth_type: AuthType::ApiKey,
            default_headers: r.header_template.clone(),
            env_http_headers: Default::default(),
            fixed_temperature: TempPolicy::Inherit,
            default_max_tokens: Some(4096),
            fallback_models: vec![],
            supports_vision: false,
            request_max_retries: 3,
            stream_idle_timeout_ms: 30_000,
            stream_ttfb_timeout_ms: 60_000,
            cache_supported: false,
            hooks: Box::new(DefaultHooks),
        }));
    }
    match reg.get(provider) {
        Some(p) => ProfileSource::Static(p),
        None => ProfileSource::DbRow(Box::new(ProviderProfile {
            name: provider.to_string(),
            api_mode: ApiMode::ChatCompletions,
            base_url: String::new(),
            models_url: None,
            auth_type: AuthType::ApiKey,
            default_headers: Default::default(),
            env_http_headers: Default::default(),
            fixed_temperature: TempPolicy::Inherit,
            default_max_tokens: Some(4096),
            fallback_models: vec![],
            supports_vision: false,
            request_max_retries: 3,
            stream_idle_timeout_ms: 30_000,
            stream_ttfb_timeout_ms: 60_000,
            cache_supported: false,
            hooks: Box::new(DefaultHooks),
        })),
    }
}
