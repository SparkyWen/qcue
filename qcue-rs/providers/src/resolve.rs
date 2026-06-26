// QCue S1-R8 — api_mode resolution precedence: explicit > provider-name > base_url host > default.
// Re-export the http base_url helpers so callers reach them through `providers`.
pub use http::client::{validate_base_url_security, versioned_base_url};
use protocol::ApiMode;

pub fn resolve_api_mode(explicit: Option<ApiMode>, provider: &str, base_url: &str) -> ApiMode {
    if let Some(m) = explicit {
        return m;
    }
    let p = provider.to_ascii_lowercase();
    if p == "anthropic" || p.ends_with("-anthropic") {
        return ApiMode::AnthropicMessages;
    }
    let host = base_url.trim_end_matches('/');
    if host.contains("api.anthropic.com") || host.ends_with("/anthropic") {
        return ApiMode::AnthropicMessages;
    }
    ApiMode::ChatCompletions
}

/// RESP-R2 — the effective wire for a specific (profile, model): a per-model hook override
/// (OpenAI gpt-5.x → Responses) wins over the profile's provider-level `api_mode`. This is the ONE
/// place the harness asks "which wire does THIS model speak", so `transport_for` stays the single switch.
///
/// KILL-SWITCH: `QCUE_RESPONSES_API=0|false|off` pins every model back to its provider-level wire
/// (gpt-5.x → the chat/completions stop-gap), for INSTANT prod rollback without a rebuild if the live
/// Responses wire misbehaves. Default ON.
pub fn effective_api_mode(profile: &crate::profile::ProviderProfile, model: &str) -> ApiMode {
    if responses_api_enabled()
        && let Some(m) = profile.hooks.api_mode_override(model)
    {
        return m;
    }
    profile.api_mode
}

/// The Responses-API kill-switch (default ON). `QCUE_RESPONSES_API=0|false|off|no` disables per-model
/// wire overrides so gpt-5.x falls back to the chat/completions stop-gap.
fn responses_api_enabled() -> bool {
    match std::env::var("QCUE_RESPONSES_API") {
        Ok(v) => !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "off" | "no"),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::registry::register_all;

    #[test]
    fn gpt5x_resolves_to_responses_others_to_chat() {
        let reg = register_all();
        let p = reg.get("openai").unwrap();
        assert_eq!(effective_api_mode(p, "gpt-5.5"), ApiMode::Responses);
        assert_eq!(effective_api_mode(p, "gpt-5.4-mini"), ApiMode::Responses);
        assert_eq!(effective_api_mode(p, "gpt-5"), ApiMode::ChatCompletions, "base gpt-5 stays on chat");
        assert_eq!(effective_api_mode(p, "gpt-4o"), ApiMode::ChatCompletions);
        assert_eq!(effective_api_mode(p, "o3"), ApiMode::ChatCompletions);
        let a = reg.get("anthropic").unwrap();
        assert_eq!(effective_api_mode(a, "claude-opus-4-8"), ApiMode::AnthropicMessages);
    }
}
