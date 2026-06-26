// QCue S1-R9, S1-R11 — declarative provider profile. Owns NO client/credential/stream state.
use crate::hooks::ProviderHooks;
use protocol::ApiMode;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AuthType {
    ApiKey,
    OAuthExternal,
    AwsSdk,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TempPolicy {
    Inherit,
    Fixed(f32),
    Omit,
}

pub struct ProviderProfile {
    pub name: String,
    pub api_mode: ApiMode,
    pub base_url: String,
    pub models_url: Option<String>,
    pub auth_type: AuthType,
    pub default_headers: HashMap<String, String>,
    /// KEY = the HTTP header name that carries the credential (e.g. `Authorization`, `x-api-key`); the
    /// VALUE is unused legacy and is NEVER read. The secret is the per-tenant BYOK vault key resolved at
    /// dispatch (see `dispatch_http`) — no API key is ever read from the environment (S1-R9, S1-R38).
    pub env_http_headers: HashMap<String, String>,
    pub fixed_temperature: TempPolicy,
    pub default_max_tokens: Option<u32>,
    pub fallback_models: Vec<String>,
    pub supports_vision: bool,
    pub request_max_retries: u32,
    pub stream_idle_timeout_ms: u64,
    pub stream_ttfb_timeout_ms: u64,
    pub cache_supported: bool,
    pub hooks: Box<dyn ProviderHooks>,
}
