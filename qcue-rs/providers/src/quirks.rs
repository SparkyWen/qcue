// QCue S1-R3, S1-R71..R78 — wire-quirk tables as tested data. Q1-Q8 from the §12 table.
use crate::hooks::Effort;
use serde::{Deserialize, Serialize};

/// Q3 accept-list: providers that accept (and DeepSeek models that need) reasoning_content replay.
const REASONING_REPLAY_PROVIDERS: &[&str] = &[
    "deepseek",
    "nvidianim",
    "nvidia",
    "openrouter",
    "xiaomimimo",
    "novita",
    "fireworks",
    "siliconflow",
    "arcee",
    "sglang",
];

fn is_deepseek_family_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.contains("deepseek")
        || m.contains("reasoner")
        || m.contains("-reasoning")
        || m.contains("-thinking")
        || m.starts_with("deepseek-r")
}

/// Q1/Q3 — replay reasoning_content byte-stable when (provider accepts it) AND (model is a reasoning model).
pub fn needs_reasoning_replay(provider: &str, model: &str) -> bool {
    let p = provider.to_ascii_lowercase();
    REASONING_REPLAY_PROVIDERS.contains(&p.as_str()) && is_deepseek_family_model(model)
}

/// Q7 — providers that natively stream reasoning in reasoning_content.
pub fn is_reasoning_provider(provider: &str) -> bool {
    matches!(
        provider.to_ascii_lowercase().as_str(),
        "deepseek" | "siliconflow" | "novita" | "fireworks"
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinkingDisable {
    ThinkingTypeDisabled,            // {"thinking":{"type":"disabled"}}
    ChatTemplateEnableThinkingFalse, // chat_template_kwargs.enable_thinking=false (vLLM)
    ChatTemplateThinkingFalse,       // chat_template_kwargs.thinking=false (Nvidia)
    Ignored,                         // OpenAI/Moonshot/Ollama ignore
}

/// Q4 — per-provider thinking-disable shape.
pub fn thinking_disable_shape(provider: &str) -> ThinkingDisable {
    match provider.to_ascii_lowercase().as_str() {
        "deepseek" | "siliconflow" | "volcengine" => ThinkingDisable::ThinkingTypeDisabled,
        "vllm" => ThinkingDisable::ChatTemplateEnableThinkingFalse,
        "nvidia" | "nvidianim" => ThinkingDisable::ChatTemplateThinkingFalse,
        _ => ThinkingDisable::Ignored,
    }
}

/// Q5 — per-vendor effort scaling.
pub fn scale_effort(provider: &str, effort: Effort) -> Effort {
    match provider.to_ascii_lowercase().as_str() {
        "deepseek" | "siliconflow" => match effort {
            Effort::Low | Effort::Medium => Effort::High,
            Effort::XHigh => Effort::Max,
            other => other,
        },
        "vllm" => match effort {
            Effort::Max => Effort::High,
            other => other,
        },
        _ => effort, // openrouter passthrough + default
    }
}

/// String form for a body value.
pub fn effort_str(e: Effort) -> &'static str {
    match e {
        Effort::Minimal => "minimal",
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::XHigh => "xhigh",
        Effort::Max => "max",
    }
}

/// Generic OpenAiCompatible quirk flags carried by a DB-row provider.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct QuirkFlags {
    pub accepts_reasoning_content: bool,
    pub usage_uses_prompt_tokens_names: bool,
}
