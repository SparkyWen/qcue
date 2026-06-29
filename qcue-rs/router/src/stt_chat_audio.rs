// QCue S1-R79..R82 — Family B: speech-to-text via an OpenAI-compatible `chat/completions` call that
// carries the audio as an `input_audio` content part. One impl serves Qwen (`qwen3-asr-flash`) and
// Gemini (OpenAI-compat layer). Envelope-never-raise; the BYOK key lives only in the auth header.
use crate::stt::TranscriptionProvider;
use crate::stt_openai::detect_audio_format;
use async_trait::async_trait;
use base64::Engine;
use protocol::TranscriptionResult;

pub struct ChatAudioTranscriptionProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    default_model: String,
    provider_name: String,
    /// When true, send ONLY the `input_audio` content part — no text instruction. See `audio_only`.
    audio_only: bool,
}

impl ChatAudioTranscriptionProvider {
    pub fn new(
        client: reqwest::Client,
        api_key: String,
        base_url: impl Into<String>,
        default_model: impl Into<String>,
        provider_name: impl Into<String>,
    ) -> Self {
        Self {
            client,
            api_key,
            base_url: base_url.into(),
            default_model: default_model.into(),
            provider_name: provider_name.into(),
            audio_only: false,
        }
    }

    /// Send ONLY the `input_audio` content part, with no text-instruction part. A *dedicated* ASR
    /// model — Qwen `qwen3-asr-flash` — rejects any non-audio part (`<400> … the dedicated task
    /// `asr` … does not support this input`); a *general* multimodal model (Gemini) needs the
    /// "transcribe this" instruction. Default `false` (instruction included). Set `true` for Qwen.
    pub fn audio_only(mut self, yes: bool) -> Self {
        self.audio_only = yes;
        self
    }

    fn fail(&self, msg: String) -> TranscriptionResult {
        TranscriptionResult {
            success: false,
            transcript: String::new(),
            error: Some(msg),
            provider: self.provider_name.clone(),
        }
    }
}

#[async_trait]
impl TranscriptionProvider for ChatAudioTranscriptionProvider {
    fn name(&self) -> &str {
        &self.provider_name
    }
    fn default_model(&self) -> Option<&str> {
        Some(&self.default_model)
    }

    async fn transcribe(
        &self,
        audio: &[u8],
        model: Option<&str>,
        language: Option<&str>,
    ) -> TranscriptionResult {
        let model = model.unwrap_or(&self.default_model);
        // Sniff the container so the `format` tag + data-URL mime are honest.
        let fmt = detect_audio_format(audio);
        let b64 = base64::engine::general_purpose::STANDARD.encode(audio);
        let data_url = format!("data:{};base64,{}", fmt.mime, b64);
        // The audio part is always present. The text-instruction part is sent ONLY for general
        // multimodal models (Gemini); a dedicated ASR model (Qwen) 400s on any non-audio part.
        let mut content = vec![serde_json::json!(
            {"type": "input_audio", "input_audio": {"data": data_url, "format": fmt.kind}}
        )];
        if !self.audio_only {
            let lang_hint = language
                .filter(|l| !l.is_empty())
                .map(|l| format!(" The spoken language is {l}."))
                .unwrap_or_default();
            content.push(serde_json::json!({"type": "text", "text": format!(
                "Transcribe this audio verbatim. Return only the transcript text, no commentary.{lang_hint}")}));
        }
        let body = serde_json::json!({
            "model": model,
            "messages": [{ "role": "user", "content": content }]
        });
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = match self.client.post(&url).bearer_auth(&self.api_key).json(&body).send().await {
            Ok(r) => r,
            Err(e) => return self.fail(format!("{} stt transport: {e}", self.provider_name)),
        };
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return self.fail(format!("{} stt {}: {}", self.provider_name, status.as_u16(), text));
        }
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => {
                let transcript = v
                    .pointer("/choices/0/message/content")
                    .and_then(|c| c.as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if transcript.is_empty() {
                    return self
                        .fail(format!("{} stt returned an empty transcript", self.provider_name));
                }
                TranscriptionResult {
                    success: true,
                    transcript,
                    error: None,
                    provider: self.provider_name.clone(),
                }
            }
            Err(e) => self.fail(format!("{} stt decode: {e}", self.provider_name)),
        }
    }
}
