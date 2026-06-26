// QCue S1-R10 — Hermes's 6 hooks. Object-safe (dyn), so #[async_trait] (S1-R6).
use async_trait::async_trait;
use protocol::Message;
use serde_json::{Map, Value, json};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effort {
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

#[derive(Clone, Debug, Default)]
pub struct ReasoningConfig {
    pub effort: Option<Effort>,
}

#[derive(Clone, Debug, Default)]
pub struct ReqCtx {
    pub session_id: Option<String>,
    pub model: String,
}

#[async_trait]
pub trait ProviderHooks: Send + Sync {
    fn prepare_messages(&self, m: Vec<Message>) -> Vec<Message> {
        m
    }
    fn build_extra_body(&self, _session: Option<&str>, _ctx: &ReqCtx) -> Value {
        json!({})
    }
    fn build_api_kwargs_extras(
        &self,
        _reasoning: Option<&ReasoningConfig>,
        _ctx: &ReqCtx,
    ) -> (Value, Map<String, Value>) {
        (json!({}), Map::new())
    }
    fn get_max_tokens(&self, _model: &str) -> Option<u32> {
        None
    }
    async fn fetch_models(&self, _key: Option<&str>, _ttl: Duration) -> Option<Vec<String>> {
        None
    }
    fn apply_reasoning_effort(&self, _body: &mut Value, _effort: Effort) {}

    /// RESP-R2 — per-MODEL api_mode override. Default `None` ⇒ use the profile's provider-level
    /// `api_mode`. A provider whose models split across wires (OpenAI: gpt-4o→chat, gpt-5.x→responses)
    /// returns `Some(_)` for the models that diverge. Resolved by `crate::resolve::effective_api_mode`.
    fn api_mode_override(&self, _model: &str) -> Option<protocol::ApiMode> {
        None
    }
}

/// The neutral default impl: every hook is a no-op.
pub struct DefaultHooks;
#[async_trait]
impl ProviderHooks for DefaultHooks {}
