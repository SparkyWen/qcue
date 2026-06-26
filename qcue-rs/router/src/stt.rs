// QCue S1-R79..R82 — TranscriptionProvider trait + STT router. Envelope-never-raise; shared fallback.
use async_trait::async_trait;
use protocol::TranscriptionResult;

#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    fn name(&self) -> &str;
    fn is_available(&self) -> bool {
        true
    }
    fn default_model(&self) -> Option<&str> {
        None
    }
    /// NEVER errors — any failure becomes {success:false, error, provider}.
    async fn transcribe(
        &self,
        audio: &[u8],
        model: Option<&str>,
        language: Option<&str>,
    ) -> TranscriptionResult;
}

#[derive(Clone, Copy, Debug)]
pub struct AudioConstraints {
    pub max_bytes: usize,
    pub max_seconds: u32,
}
impl Default for AudioConstraints {
    fn default() -> Self {
        Self { max_bytes: 25 * 1024 * 1024, max_seconds: 600 }
    }
}

/// Routes STT down the configured provider chain (cloud fallback to on-device STT, D4).
pub struct SttRouter {
    providers: Vec<Box<dyn TranscriptionProvider>>,
    constraints: AudioConstraints,
}
impl SttRouter {
    pub fn new(providers: Vec<Box<dyn TranscriptionProvider>>) -> Self {
        Self { providers, constraints: AudioConstraints::default() }
    }
    pub fn with_constraints(mut self, c: AudioConstraints) -> Self {
        self.constraints = c;
        self
    }

    /// S1-R81 — validate constraints BEFORE any network call; S1-R82 — try providers in order.
    pub async fn transcribe(
        &self,
        audio: &[u8],
        model: Option<&str>,
        language: Option<&str>,
    ) -> TranscriptionResult {
        if audio.len() > self.constraints.max_bytes {
            return TranscriptionResult {
                success: false,
                transcript: String::new(),
                error: Some(format!(
                    "audio constraint violated: {} > {} bytes",
                    audio.len(),
                    self.constraints.max_bytes
                )),
                provider: "router".into(),
            };
        }
        let mut last = TranscriptionResult {
            success: false,
            transcript: String::new(),
            error: Some("no STT provider configured".into()),
            provider: "router".into(),
        };
        for p in &self.providers {
            if !p.is_available() {
                continue;
            }
            let r = p.transcribe(audio, model, language).await;
            if r.success {
                return r;
            }
            last = r; // S1-R80 — fall through the shared chain on failure
        }
        last
    }
}
