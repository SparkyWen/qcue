// QCue S1-R13 — OpenAI profile (M1 wired).
use crate::hooks::{Effort, ProviderHooks};
use crate::profile::{AuthType, ProviderProfile, TempPolicy};
use protocol::ApiMode;
use serde_json::Value;

/// OpenAI reasoning models accept `reasoning_effort`; plain chat models (gpt-4*) reject it.
/// Reasoning-capable = the gpt-5 family or the o-series (o1/o3/o4…).
fn model_supports_reasoning(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    if m.starts_with("gpt-5") {
        return true;
    }
    // o-series: an 'o' followed immediately by a digit (o1, o3, o4-mini).
    m.starts_with('o') && m.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
}

/// Whether this OpenAI model accepts the `reasoning_effort` param on the **/v1/chat/completions** wire.
///
/// The original gpt-5 generation (gpt-5, gpt-5-mini/-nano) and the o-series (o1/o3/o4…) accept it on
/// chat completions. The newer **gpt-5.x dot-minor** generation (gpt-5.1 / 5.4 / 5.5 …) moved reasoning
/// control to the **/v1/responses** API and REJECT `reasoning_effort` on chat completions — with
/// function tools present OpenAI returns HTTP 400:
///   "Function tools with reasoning_effort are not supported for gpt-5.5 in /v1/chat/completions"
/// (it advises /v1/responses). QCue is chat-completions-only for OpenAI (Responses is M6+/NG1) and
/// every recall/Dream turn advertises function tools, so emitting reasoning_effort for that generation
/// 400s the turn → tool calling + web (联网) silently break. We therefore must NOT send it for gpt-5.x.
/// (The model still reasons at its server default — gpt-5.5 defaults to "medium".)
/// See docs/postmortems/2026-06-19-gpt5x-reasoning-effort-breaks-tools.md.
fn accepts_chat_reasoning_effort(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    if m.starts_with("gpt-5.") {
        return false; // gpt-5.x dot-minor → /v1/responses only for reasoning_effort.
    }
    model_supports_reasoning(&m)
}

/// Map QCue's 6 effort levels onto OpenAI's `reasoning_effort` tokens. OpenAI's top tier is
/// `high`, so XHigh/Max clamp down. `minimal` only exists on gpt-5; the o-series clamps it to `low`.
fn openai_effort_str(model: &str, effort: Effort) -> &'static str {
    let minimal_ok = model.to_ascii_lowercase().starts_with("gpt-5");
    match effort {
        Effort::Minimal => {
            if minimal_ok {
                "minimal"
            } else {
                "low"
            }
        }
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High | Effort::XHigh | Effort::Max => "high",
    }
}

pub struct OpenAiHooks;

impl ProviderHooks for OpenAiHooks {
    fn apply_reasoning_effort(&self, body: &mut Value, effort: Effort) {
        let model = body.get("model").and_then(Value::as_str).unwrap_or_default().to_string();
        if !accepts_chat_reasoning_effort(&model) {
            // Either a non-reasoning chat model (gpt-4*) or the gpt-5.x dot-minor generation, which
            // rejects reasoning_effort on /v1/chat/completions (esp. with function tools → 400; it
            // needs /v1/responses, which QCue doesn't speak yet). Skip it so the recall/web (联网) tool
            // turn survives — the model still reasons at its server default.
            return;
        }
        body["reasoning_effort"] = Value::String(openai_effort_str(&model, effort).to_string());
    }

    fn api_mode_override(&self, model: &str) -> Option<ApiMode> {
        // RESP-R2 — the gpt-5.x dot-minor generation routes to /v1/responses, where reasoning effort and
        // function tools coexist (chat/completions 400s the combo). gpt-4o / base gpt-5 / o-series stay on
        // chat/completions (they already work there). See the Responses-API transport spec (D19).
        model.to_ascii_lowercase().starts_with("gpt-5.").then_some(ApiMode::Responses)
    }
}

pub fn profile() -> ProviderProfile {
    let mut env_http_headers = std::collections::HashMap::new();
    env_http_headers.insert("Authorization".into(), "OPENAI_API_KEY".into());
    ProviderProfile {
        name: "openai".into(),
        api_mode: ApiMode::ChatCompletions,
        base_url: "https://api.openai.com/v1".into(),
        models_url: Some("https://api.openai.com/v1/models".into()),
        auth_type: AuthType::ApiKey,
        default_headers: Default::default(),
        env_http_headers,
        fixed_temperature: TempPolicy::Inherit,
        default_max_tokens: Some(4096),
        fallback_models: vec!["gpt-5.4-mini".into()], // RESP-R11 — current catalog low-price id (was gpt-4o-mini, delisted)
        supports_vision: true,
        request_max_retries: 3,
        stream_idle_timeout_ms: 30_000,
        stream_ttfb_timeout_ms: 60_000,
        cache_supported: true,
        hooks: Box::new(OpenAiHooks),
    }
}
