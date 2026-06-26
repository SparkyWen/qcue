// QCue S1-R17 — Transport trait + NormalizedResponse non-stream normalization contract.
use protocol::{ApiMode, FinishReason, Message, NormalizedResponse, ToolDef, TransportError};
use providers::profile::ProviderProfile;
use serde_json::Value;

/// A provider-native (server-executed) tool. Unlike a `ToolDef` (a client function the harness runs),
/// the PROVIDER runs these and returns results inline — the harness never executes them. Each transport
/// renders this into the right wire shape (F-1). Start with web search; extend as providers add tools.
#[derive(Clone, Debug, PartialEq)]
pub enum ServerTool {
    /// Provider web search. Anthropic: `web_search_20260209`; OpenAI chat: `web_search_options`.
    WebSearch { max_uses: Option<u32> },
}

/// Per-call request params the caller supplies (temperature, max_tokens, response_format).
#[derive(Clone, Debug, Default)]
pub struct ReqParams {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub response_format: Option<Value>, // structured output (S1-R57)
    pub stream: bool,
    /// Provider-native server tools (e.g. web search). The PROVIDER executes them; results return inline,
    /// never as a client tool round-trip. Empty by default — recall/Dream stay no-network (RKM §7.7).
    pub server_tools: Vec<ServerTool>,
    /// Per-provider reasoning effort. The provider hooks map it to the right wire (DeepSeek/Kimi:
    /// `thinking:{disabled}` for Minimal, else `reasoning_effort`). `None` ⇒ the provider default.
    pub reasoning: Option<providers::hooks::ReasoningConfig>,
    /// A stable session id for provider prompt-cache keys (`build_extra_body`). `None` until a caller
    /// threads one through (the hook is a no-op for providers that don't use it).
    pub session_id: Option<String>,
}

pub trait Transport: Send + Sync {
    fn api_mode(&self) -> ApiMode;
    fn convert_messages(&self, msgs: &[Message], model: &str) -> Value;
    fn convert_tools(&self, tools: &[ToolDef]) -> Value;
    fn build_kwargs(
        &self,
        model: &str,
        msgs: &[Message],
        tools: Option<&[ToolDef]>,
        profile: &ProviderProfile,
        params: &ReqParams,
    ) -> Value;
    fn normalize_response(&self, raw: &Value) -> Result<NormalizedResponse, TransportError>;
    fn validate_response(&self, _raw: &Value) -> bool {
        true
    }
    fn map_finish_reason(&self, _raw: &str) -> FinishReason {
        FinishReason::Stop
    }
}

/// S1-R44/R89 — the ONLY place api_mode is matched. The loop & dispatch reach a wire transport
/// only through this factory, so neither grows a per-provider branch.
pub fn transport_for(api_mode: ApiMode) -> Box<dyn Transport> {
    use crate::transport_anthropic::AnthropicTransport;
    use crate::transport_chat::ChatCompletionsTransport;
    use crate::transport_responses::ResponsesTransport;
    match api_mode {
        ApiMode::ChatCompletions => Box::new(ChatCompletionsTransport),
        ApiMode::AnthropicMessages => Box::new(AnthropicTransport),
        ApiMode::Responses => Box::new(ResponsesTransport), // D19 — OpenAI /v1/responses (gpt-5.x)
    }
}
