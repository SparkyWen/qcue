// QCue S1-R13 — provider identity.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The seven launch providers (D7). The enum captures BEHAVIOR; long-tail vendors are DB rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum FirstClassProvider {
    OpenAi,
    Anthropic,
    Gemini,
    DeepSeek,
    Kimi,
    Qwen,
    OpenRouter,
}

/// A provider id as it appears in `provider_credentials.provider` and profiles.
pub type ProviderId = String;
