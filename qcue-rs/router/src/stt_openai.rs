// QCue S1-R79..R82 — the OpenAI cloud STT provider (gpt-4o-mini-transcribe / whisper). Implements the
// `TranscriptionProvider` trait so it slots into the shared `SttRouter` (constraint check + fallback).
// Envelope-never-raise: any transport/HTTP/parse failure becomes {success:false, error}, never a panic.
// The API key lives only in the Authorization header for the duration of the call (never logged).
use crate::stt::TranscriptionProvider;
use async_trait::async_trait;
use protocol::TranscriptionResult;

/// OpenAI's default transcription model. `gpt-4o-mini-transcribe-2025-12-15` is OpenAI's recommended
/// transcription model (lower WER + ~89% fewer hallucinations than whisper-1); pinned to a dated
/// snapshot so a slug repoint can't silently change behavior. Operators override it WITHOUT a rebuild
/// via `QCUE_OPENAI_TRANSCRIBE_MODEL` (wired at the `app-server` `OpenAiTranscriber` layer).
pub const DEFAULT_TRANSCRIBE_MODEL: &str = "gpt-4o-mini-transcribe-2025-12-15";

pub struct OpenAiTranscriptionProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    default_model: String,
}

impl OpenAiTranscriptionProvider {
    /// `api_key` is the tenant's decrypted BYOK OpenAI key (caller owns its zeroizing lifetime).
    pub fn new(client: reqwest::Client, api_key: String) -> Self {
        Self {
            client,
            api_key,
            base_url: "https://api.openai.com/v1".into(),
            default_model: DEFAULT_TRANSCRIBE_MODEL.into(),
        }
    }

    /// Override the endpoint base (tests point this at a mock server).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Override the default transcription model (env-driven at the app-server layer). The per-call
    /// `model` argument still wins when present; this only sets the fallback used when it is `None`.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
        self
    }

    fn fail(msg: String) -> TranscriptionResult {
        TranscriptionResult {
            success: false,
            transcript: String::new(),
            error: Some(msg),
            provider: "openai".into(),
        }
    }
}

/// A detected audio container: the multipart `file_name` (OpenAI keys format detection off the filename
/// EXTENSION first) and the `mime` to label the upload with. `kind` is a short, log-safe tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioFormat {
    pub kind: &'static str,
    pub file_name: &'static str,
    pub mime: &'static str,
}

/// The QCue app records AAC-in-MP4 (`.m4a`); use it as the fallback for unrecognized bytes so the
/// common case is unchanged from the previous hardcoded label.
pub const AUDIO_M4A: AudioFormat =
    AudioFormat { kind: "m4a", file_name: "audio.m4a", mime: "audio/mp4" };

/// Sniff the audio container from leading magic bytes so the OpenAI multipart carries an HONEST
/// filename + MIME. OpenAI detects the format from the filename extension first, so mislabeling
/// non-MP4 bytes as `audio.m4a` (the old hardcoded behavior) yields an opaque
/// "Audio file might be corrupted or unsupported" 400. A correct extension either transcribes the
/// audio or produces an actionable "unsupported <fmt>" error instead. Unknown bytes fall back to m4a.
pub fn detect_audio_format(bytes: &[u8]) -> AudioFormat {
    let b = bytes;
    // ISO base-media family (MP4 / M4A / 3GP): a `....ftyp` box at offset 4.
    if b.len() >= 12 && &b[4..8] == b"ftyp" {
        if b[8..10].eq_ignore_ascii_case(b"3g") {
            return AudioFormat { kind: "3gp", file_name: "audio.3gp", mime: "audio/3gpp" };
        }
        return AUDIO_M4A; // m4a / mp4 / isom / M4A — the AAC-in-MP4 the app records
    }
    // RIFF/WAVE
    if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WAVE" {
        return AudioFormat { kind: "wav", file_name: "audio.wav", mime: "audio/wav" };
    }
    // Ogg (vorbis/opus)
    if b.starts_with(b"OggS") {
        return AudioFormat { kind: "ogg", file_name: "audio.ogg", mime: "audio/ogg" };
    }
    // EBML → WebM / Matroska
    if b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return AudioFormat { kind: "webm", file_name: "audio.webm", mime: "audio/webm" };
    }
    // FLAC
    if b.starts_with(b"fLaC") {
        return AudioFormat { kind: "flac", file_name: "audio.flac", mime: "audio/flac" };
    }
    // CoreAudio (CAF) — iOS *could* produce this; OpenAI does NOT accept it, but an honest `.caf`
    // label yields an actionable "unsupported caf" instead of a misleading "corrupted m4a".
    if b.starts_with(b"caff") {
        return AudioFormat { kind: "caf", file_name: "audio.caf", mime: "audio/x-caf" };
    }
    // AMR
    if b.starts_with(b"#!AMR") {
        return AudioFormat { kind: "amr", file_name: "audio.amr", mime: "audio/amr" };
    }
    // MP3: an ID3 tag or an MPEG-audio frame sync (0xFFEx/0xFFFx). The app never emits raw ADTS, so
    // `.mp3` is the safe supported label for a bare MPEG-audio stream.
    if b.starts_with(b"ID3") || (b.len() >= 2 && b[0] == 0xFF && (b[1] & 0xE0) == 0xE0) {
        return AudioFormat { kind: "mp3", file_name: "audio.mp3", mime: "audio/mpeg" };
    }
    AUDIO_M4A
}

/// A redaction-safe fingerprint of the audio head for diagnostics: lowercase hex of up to the first
/// `n` bytes. These are CONTAINER HEADER bytes (e.g. `....ftypM4A `), never user speech or secrets.
pub fn audio_head_hex(bytes: &[u8], n: usize) -> String {
    let take = bytes.len().min(n);
    let mut s = String::with_capacity(take * 2);
    for byte in &bytes[..take] {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

#[async_trait]
impl TranscriptionProvider for OpenAiTranscriptionProvider {
    fn name(&self) -> &str {
        "openai"
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
        let model = model.unwrap_or(&self.default_model).to_string();
        // Label the upload by sniffing the actual container — OpenAI keys format detection off the
        // filename extension, so a wrong label is the difference between a transcript and an opaque
        // "Audio file might be corrupted or unsupported" 400.
        let fmt = detect_audio_format(audio);
        let part = match reqwest::multipart::Part::bytes(audio.to_vec())
            .file_name(fmt.file_name)
            .mime_str(fmt.mime)
        {
            Ok(p) => p,
            Err(e) => return Self::fail(format!("openai stt: bad audio part: {e}")),
        };
        let mut form = reqwest::multipart::Form::new().text("model", model).part("file", part);
        if let Some(lang) = language.filter(|l| !l.is_empty()) {
            form = form.text("language", lang.to_string());
        }
        let url = format!("{}/audio/transcriptions", self.base_url);
        let resp = self.client.post(&url).bearer_auth(&self.api_key).multipart(form).send().await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => return Self::fail(format!("openai stt transport: {e}")),
        };
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            // OpenAI's "corrupted or unsupported" / "unsupported file format" 400 means the *bytes*
            // (not the key/model) are the problem — almost always a too-short clip or a recording that
            // didn't finalize. Map it to an actionable line (the app shows this verbatim) carrying the
            // size + detected container so the cause is diagnosable from the message alone.
            let lower = body.to_lowercase();
            if status.as_u16() == 400
                && (lower.contains("corrupt") || lower.contains("unsupported"))
            {
                return Self::fail(format!(
                    "the recording couldn't be read by the speech service \
                     ({} bytes, detected {}). It was likely too short or didn't finish recording — \
                     hold the mic and speak for at least a second, then try again.",
                    audio.len(),
                    fmt.kind,
                ));
            }
            return Self::fail(format!("openai stt {}: {}", status.as_u16(), body));
        }
        match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) => {
                let text =
                    v.get("text").and_then(|t| t.as_str()).unwrap_or_default().trim().to_string();
                TranscriptionResult {
                    success: true,
                    transcript: text,
                    error: None,
                    provider: "openai".into(),
                }
            }
            Err(e) => Self::fail(format!("openai stt decode: {e}")),
        }
    }
}
