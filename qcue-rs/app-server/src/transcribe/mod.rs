//! QCue D4 — cloud voice transcription. The app records audio and POSTs it to `/v1/transcribe`; the
//! server transcribes it with the tenant's BYOK OpenAI key (gpt-4o-mini-transcribe) and returns the text,
//! which the app drops into an editable compose field for review before capture.
//!
//! The route never branches on stub-vs-real: it calls a per-tenant `Transcriber` seam (mirrors the
//! `recall_llm`/`ingest_llm` pattern). Tests inject `StubTranscriber`; prod injects `OpenAiTranscriber`,
//! which loads + unseals the tenant's OpenAI key through the SAME `Secrets` vault used by dispatch — the
//! plaintext lives only for the duration of the HTTP call and is never logged or persisted (S1-R38/B-R13).
pub mod routes;

use crate::vault::secrets::{SealedKey, Secrets};
use async_trait::async_trait;
use protocol::TranscriptionResult;
use router::stt::SttRouter;
use router::stt_openai::OpenAiTranscriptionProvider;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

/// The per-tenant transcription seam. The route depends only on this trait.
#[async_trait]
pub trait Transcriber: Send + Sync {
    async fn transcribe(
        &self,
        tenant: Uuid,
        audio: &[u8],
        model: Option<&str>,
        language: Option<&str>,
    ) -> TranscriptionResult;
}

/// Keyless test/demo double: returns a fixed transcript, ignoring the tenant/key.
pub struct StubTranscriber {
    transcript: String,
}
impl StubTranscriber {
    pub fn new(transcript: impl Into<String>) -> Self {
        Self { transcript: transcript.into() }
    }
}
#[async_trait]
impl Transcriber for StubTranscriber {
    async fn transcribe(
        &self,
        _tenant: Uuid,
        _audio: &[u8],
        _model: Option<&str>,
        _language: Option<&str>,
    ) -> TranscriptionResult {
        TranscriptionResult {
            success: true,
            transcript: self.transcript.clone(),
            error: None,
            provider: "stub".into(),
        }
    }
}

/// Production transcriber: loads the tenant's OpenAI BYOK key and calls the cloud STT provider through
/// the shared `SttRouter` (constraint check + envelope-never-raise).
pub struct OpenAiTranscriber {
    pool: PgPool,
    secrets: Arc<dyn Secrets>,
    client: reqwest::Client,
    /// Optional model override from `QCUE_OPENAI_TRANSCRIBE_MODEL`; `None` ⇒ the provider's compiled
    /// default (`DEFAULT_TRANSCRIBE_MODEL`). Lets ops follow an OpenAI model rotation with a restart.
    model_override: Option<String>,
}

impl OpenAiTranscriber {
    pub fn new(pool: PgPool, secrets: Arc<dyn Secrets>) -> Self {
        let (opts, _allow_insecure) = http::client::opts_from_env();
        let client = http::client::build_client(opts).unwrap_or_default();
        let model_override = std::env::var("QCUE_OPENAI_TRANSCRIBE_MODEL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Self { pool, secrets, client, model_override }
    }

    /// Load + unseal the tenant's highest-priority, non-dead OpenAI key (RLS-bound tx). Returns the
    /// plaintext in a zeroize-on-drop buffer; `None` when no usable OpenAI key is configured.
    async fn open_openai_key(&self, tenant: Uuid) -> Option<crate::vault::secrets::Zeroizing> {
        let mut tx = self.pool.begin().await.ok()?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .ok()?;
        let row = sqlx::query(
            "SELECT key_ciphertext,key_nonce,key_tag,dek_wrapped,kek_id,key_hint \
             FROM provider_credentials \
             WHERE provider='openai' AND status <> 'dead' \
             ORDER BY priority LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await
        .ok()??;
        tx.commit().await.ok()?;
        let sealed = SealedKey {
            key_ciphertext: row.get("key_ciphertext"),
            key_nonce: row.get("key_nonce"),
            key_tag: row.get("key_tag"),
            dek_wrapped: row.get("dek_wrapped"),
            kek_id: row.get("kek_id"),
            key_hint: row.get("key_hint"),
        };
        self.secrets.open(tenant, &sealed).await.ok()
    }
}

#[async_trait]
impl Transcriber for OpenAiTranscriber {
    async fn transcribe(
        &self,
        tenant: Uuid,
        audio: &[u8],
        model: Option<&str>,
        language: Option<&str>,
    ) -> TranscriptionResult {
        let Some(key) = self.open_openai_key(tenant).await else {
            return TranscriptionResult {
                success: false,
                transcript: String::new(),
                error: Some("no OpenAI key configured — add one in Settings to use voice".into()),
                provider: "openai".into(),
            };
        };
        // The plaintext is copied into the request header for this call only, then `key` drops (zeroed).
        let mut provider =
            OpenAiTranscriptionProvider::new(self.client.clone(), key.as_str().to_string());
        if let Some(m) = &self.model_override {
            provider = provider.with_model(m.clone());
        }
        let stt = SttRouter::new(vec![Box::new(provider)]);
        stt.transcribe(audio, model, language).await
    }
}
