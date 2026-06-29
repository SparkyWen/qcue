#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue — the STT capability registry: vendor families, per-vendor audio limits, model auto-correction.
use router::stt_vendors::{is_stt_capable, resolve_model, stt_vendor, SttKind};

#[test]
fn known_vendors_resolve_with_their_family_and_limits() {
    let z = stt_vendor("zhipu").unwrap();
    assert_eq!(z.kind, SttKind::Multipart);
    assert_eq!(z.default_model, "glm-asr-2512");
    assert_eq!(z.max_seconds, 30); // Zhipu's tighter cap
    assert_eq!(stt_vendor("qwen").unwrap().kind, SttKind::ChatAudio);
    assert!(stt_vendor("qwen").unwrap().audio_only, "Qwen is a dedicated ASR model (no text part)");
    assert!(!stt_vendor("gemini").unwrap().audio_only, "Gemini needs the text instruction");
    assert!(stt_vendor("minimax").is_none(), "MiniMax removed from the STT list for now");
}

#[test]
fn deepseek_and_unknown_are_not_stt_capable() {
    assert!(!is_stt_capable("deepseek"));
    assert!(!is_stt_capable("anthropic"));
    assert!(stt_vendor("nope").is_none());
    assert!(is_stt_capable("OpenAI")); // case-insensitive
}

#[test]
fn model_autocorrection_swaps_cross_vendor_models() {
    let groq = stt_vendor("groq").unwrap();
    assert_eq!(resolve_model(groq, Some("gpt-4o-mini-transcribe-2025-12-15")), "whisper-large-v3-turbo");
    assert_eq!(resolve_model(groq, Some("whisper-large-v3")), "whisper-large-v3");
    assert_eq!(resolve_model(groq, None), "whisper-large-v3-turbo");
}
