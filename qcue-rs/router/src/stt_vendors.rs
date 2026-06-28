// QCue S1-R79..R82 — the STT capability registry: which vendors offer speech-to-text, their wire
// family, default model, and per-vendor audio limits. Mirrors Hermes's BUILTIN_STT_PROVIDERS +
// per-vendor config. Lives in `router/` (NOT the chat `providers/` profiles): STT is a separate
// concern from chat (Hermes's decision), which also keeps the crate-layering law clean.
use crate::stt::AudioConstraints;

/// The wire-encoding family a vendor's transcription endpoint speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SttKind {
    /// `POST {base}/audio/transcriptions`, multipart `file`+`model` (OpenAI, Groq, Zhipu).
    Multipart,
    /// `POST {base}/chat/completions` with an `input_audio` content part (Qwen, Gemini).
    ChatAudio,
    /// MiniMax's vendor-native JSON ASR (Bearer + ?GroupId).
    MiniMax,
}

/// A speech-to-text-capable vendor: how to reach it and its audio limits.
#[derive(Clone, Copy, Debug)]
pub struct SttVendor {
    /// Stable id == the `provider_credentials.provider` string whose BYOK key we load.
    pub id: &'static str,
    pub kind: SttKind,
    pub base_url: &'static str,
    pub default_model: &'static str,
    /// Per-vendor audio limits (Zhipu 30s/25MB, Qwen 10MB) — checked pre-call (S1-R81).
    pub max_bytes: usize,
    pub max_seconds: u32,
}

impl SttVendor {
    pub fn constraints(&self) -> AudioConstraints {
        AudioConstraints { max_bytes: self.max_bytes, max_seconds: self.max_seconds }
    }
}

const MB: usize = 1024 * 1024;

/// The compiled STT capability table. Order == AUTO-DERIVE PRIORITY when no explicit setting:
/// a tenant's highest-priority configured BYOK key among these wins.
pub const STT_VENDORS: &[SttVendor] = &[
    SttVendor {
        id: "openai",
        kind: SttKind::Multipart,
        base_url: "https://api.openai.com/v1",
        default_model: "gpt-4o-mini-transcribe-2025-12-15",
        max_bytes: 25 * MB,
        max_seconds: 600,
    },
    SttVendor {
        id: "groq",
        kind: SttKind::Multipart,
        base_url: "https://api.groq.com/openai/v1",
        default_model: "whisper-large-v3-turbo",
        max_bytes: 25 * MB,
        max_seconds: 600,
    },
    SttVendor {
        id: "zhipu",
        kind: SttKind::Multipart,
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        default_model: "glm-asr-2512",
        max_bytes: 25 * MB,
        max_seconds: 30,
    },
    SttVendor {
        id: "gemini",
        kind: SttKind::ChatAudio,
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        default_model: "gemini-2.5-flash",
        max_bytes: 20 * MB,
        max_seconds: 540,
    },
    SttVendor {
        id: "qwen",
        kind: SttKind::ChatAudio,
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        default_model: "qwen3-asr-flash",
        max_bytes: 10 * MB,
        max_seconds: 180,
    },
    // minimax: base_url/default_model confirmed in Task 4 step 1 (ASR docs behind console login).
    // default_model is intentionally empty until Task 4 — only the MiniMax provider may read it, and
    // its impl must not require a non-empty model. Task 5's build_provider debug_asserts non-empty for
    // every OTHER (Multipart/ChatAudio) vendor so an empty model can't silently reach the network.
    SttVendor {
        id: "minimax",
        kind: SttKind::MiniMax,
        base_url: "https://api.minimax.io/v1",
        default_model: "",
        max_bytes: 20 * MB,
        max_seconds: 600,
    },
];

/// Look up a vendor by id (case-insensitive). `None` for unknown / non-STT providers (e.g. deepseek).
pub fn stt_vendor(id: &str) -> Option<&'static SttVendor> {
    let id = id.trim();
    STT_VENDORS.iter().find(|v| v.id.eq_ignore_ascii_case(id))
}

/// True when `provider` has a speech-to-text endpoint QCue can drive.
pub fn is_stt_capable(provider: &str) -> bool {
    stt_vendor(provider).is_some()
}

/// Model auto-correction (Hermes parity): a model that is *another* vendor's default → this vendor's
/// default; otherwise keep the requested model (or fall back to this vendor's default when absent).
pub fn resolve_model<'a>(vendor: &'a SttVendor, requested: Option<&'a str>) -> &'a str {
    match requested.map(str::trim).filter(|s| !s.is_empty()) {
        Some(m) if STT_VENDORS.iter().any(|o| o.id != vendor.id && o.default_model == m) => {
            vendor.default_model
        }
        Some(m) => m,
        None => vendor.default_model,
    }
}
