// QCue S1-R79..R82 — Family C: MiniMax vendor-native JSON ASR.
//
// ⚠️ WIRE SHAPE PARTIALLY UNCONFIRMED. MiniMax's ASR docs sit behind a console login. KNOWN + tested:
// host `api.minimax.io`, `Authorization: Bearer <api_key>` + a `?GroupId=<gid>` query param, JSON,
// base64 audio, and the composite credential `{"api_key","group_id"}` parsed from the single encrypted
// vault blob (no DB migration). ASSUMED + must be confirmed against the console docs (plan Task 4 step 1)
// and validated by the `#[ignore]` live test BEFORE MiniMax is relied on in prod: the ASR endpoint PATH
// (`MINIMAX_ASR_PATH`), the request BODY keys, and the RESPONSE transcript path. Envelope-never-raise
// holds regardless of shape — a wrong guess fails into a `{success:false}` envelope, never a panic.
use crate::stt::TranscriptionProvider;
use crate::stt_openai::detect_audio_format;
use async_trait::async_trait;
use base64::Engine;
use protocol::TranscriptionResult;

/// ASSUMED ASR endpoint path under the vendor base_url — confirm against MiniMax console docs.
const MINIMAX_ASR_PATH: &str = "/audio/transcriptions";

pub struct MiniMaxTranscriptionProvider {
    client: reqwest::Client,
    /// The tenant's stored MiniMax credential: JSON `{"api_key","group_id"}` (MiniMax needs BOTH a
    /// Bearer key and a GroupId, so the single vault field carries a composite blob).
    secret_blob: String,
    base_url: String,
    default_model: String,
}

impl MiniMaxTranscriptionProvider {
    pub fn new(
        client: reqwest::Client,
        secret_blob: String,
        base_url: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self { client, secret_blob, base_url: base_url.into(), default_model: default_model.into() }
    }

    fn fail(msg: String) -> TranscriptionResult {
        TranscriptionResult {
            success: false,
            transcript: String::new(),
            error: Some(msg),
            provider: "minimax".into(),
        }
    }
}

#[async_trait]
impl TranscriptionProvider for MiniMaxTranscriptionProvider {
    fn name(&self) -> &str {
        "minimax"
    }
    fn default_model(&self) -> Option<&str> {
        if self.default_model.is_empty() { None } else { Some(&self.default_model) }
    }

    async fn transcribe(
        &self,
        audio: &[u8],
        model: Option<&str>,
        _language: Option<&str>,
    ) -> TranscriptionResult {
        // Parse the composite {api_key, group_id} credential; a bare key (no JSON) → missing group_id.
        let creds: serde_json::Value = serde_json::from_str(&self.secret_blob)
            .unwrap_or_else(|_| serde_json::json!({ "api_key": self.secret_blob }));
        let api_key = creds.get("api_key").and_then(|v| v.as_str()).unwrap_or_default();
        let group_id = creds.get("group_id").and_then(|v| v.as_str()).unwrap_or_default();
        if api_key.is_empty() {
            return Self::fail("minimax: missing api_key in stored credential".into());
        }
        if group_id.is_empty() {
            return Self::fail(
                "minimax: missing GroupId — re-add your MiniMax key with its GroupId in Settings".into(),
            );
        }

        let fmt = detect_audio_format(audio);
        let b64 = base64::engine::general_purpose::STANDARD.encode(audio);
        // ASSUMED request body — confirm keys against MiniMax console docs (Task 4 step 1).
        let mut body = serde_json::json!({ "audio": b64, "format": fmt.kind });
        let m = model.unwrap_or(&self.default_model);
        if !m.is_empty() {
            body["model"] = serde_json::Value::String(m.to_string());
        }

        let url = format!("{}{}", self.base_url.trim_end_matches('/'), MINIMAX_ASR_PATH);
        let resp = self
            .client
            .post(&url)
            .query(&[("GroupId", group_id)])
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => return Self::fail(format!("minimax stt transport: {e}")),
        };
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Self::fail(format!("minimax stt {}: {}", status.as_u16(), text));
        }
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => {
                // ASSUMED response path — confirm against MiniMax console docs.
                let transcript =
                    v.pointer("/text").and_then(|t| t.as_str()).unwrap_or_default().trim().to_string();
                if transcript.is_empty() {
                    return Self::fail("minimax stt returned an empty transcript".into());
                }
                TranscriptionResult {
                    success: true,
                    transcript,
                    error: None,
                    provider: "minimax".into(),
                }
            }
            Err(e) => Self::fail(format!("minimax stt decode: {e}")),
        }
    }
}
