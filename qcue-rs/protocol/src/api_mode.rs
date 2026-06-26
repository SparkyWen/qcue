// QCue S1-R7 — wire-protocol enum. MVP = ChatCompletions + AnthropicMessages.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum ApiMode {
    ChatCompletions,
    AnthropicMessages,
    // D19 — OpenAI /v1/responses. Carries gpt-5.x (reasoning_effort + function tools coexist here,
    // unlike chat/completions which 400s the combo). See the Responses-API transport spec.
    Responses,
    // BedrockConverse — reserved (NG1).
}
