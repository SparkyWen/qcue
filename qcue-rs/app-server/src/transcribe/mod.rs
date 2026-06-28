//! QCue D4 — cloud voice transcription. The app records audio and POSTs it to `/v1/transcribe`; the
//! server transcribes it with the tenant's selected (or auto-derived) STT BYOK provider and returns the
//! text, which the app drops into an editable compose field for review before capture.
//!
//! The route never branches on stub-vs-real: it calls a per-tenant `Transcriber` seam (mirrors the
//! `recall_llm`/`ingest_llm` pattern). Tests inject `StubTranscriber`; prod injects `RoutedTranscriber`,
//! which selects the provider (explicit per-tenant setting → else auto-derive from configured BYOK keys
//! among STT-capable vendors), loads + unseals THAT provider's key through the SAME `Secrets` vault used
//! by dispatch — the plaintext lives only for the duration of the HTTP call and is never logged or
//! persisted (S1-R38/B-R13). No silent cross-provider fallback (D4).
pub mod routes;

use crate::vault::secrets::{SealedKey, Secrets};
use async_trait::async_trait;
use protocol::TranscriptionResult;
use router::stt::{SttRouter, TranscriptionProvider};
use router::stt_chat_audio::ChatAudioTranscriptionProvider;
use router::stt_minimax::MiniMaxTranscriptionProvider;
use router::stt_openai::OpenAiTranscriptionProvider;
use router::stt_vendors::{stt_vendor, SttKind, SttVendor};
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

/// Production transcriber: resolves the tenant's STT provider (explicit `settings:stt_provider` →
/// else the highest-priority configured BYOK key among STT-capable vendors), loads + unseals THAT
/// provider's key, and runs it through the shared `SttRouter` (per-vendor constraint check +
/// envelope-never-raise). No silent cross-provider fallback.
pub struct RoutedTranscriber {
    pool: PgPool,
    secrets: Arc<dyn Secrets>,
    client: reqwest::Client,
    /// Ops override for the OpenAI transcription model (`QCUE_OPENAI_TRANSCRIBE_MODEL`), preserved from
    /// the previous OpenAI-only transcriber. Applies ONLY to the `openai` vendor; `None` ⇒ vendor default.
    openai_model_override: Option<String>,
}

impl RoutedTranscriber {
    pub fn new(pool: PgPool, secrets: Arc<dyn Secrets>) -> Self {
        let (opts, _allow_insecure) = http::client::opts_from_env();
        let client = http::client::build_client(opts).unwrap_or_default();
        let openai_model_override = std::env::var("QCUE_OPENAI_TRANSCRIBE_MODEL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Self { pool, secrets, client, openai_model_override }
    }

    /// Load + unseal the tenant's highest-priority, non-dead key for `provider` (RLS-bound tx). Returns
    /// the plaintext in a zeroize-on-drop buffer; `None` when no usable key for that provider exists.
    async fn open_key(&self, tenant: Uuid, provider: &str) -> Option<crate::vault::secrets::Zeroizing> {
        let mut tx = self.pool.begin().await.ok()?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .ok()?;
        let row = sqlx::query(
            "SELECT key_ciphertext,key_nonce,key_tag,dek_wrapped,kek_id,key_hint \
             FROM provider_credentials \
             WHERE provider=$1 AND status <> 'dead' \
             ORDER BY (status = 'ok') DESC, priority LIMIT 1",
        )
        .bind(provider)
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

    /// Distinct providers with a usable (non-dead) key, in priority order (RLS-bound tx).
    async fn configured_providers(&self, tenant: Uuid) -> Vec<String> {
        let mut tx = match self.pool.begin().await {
            Ok(t) => t,
            Err(_) => return vec![],
        };
        if sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .is_err()
        {
            return vec![];
        }
        let rows = sqlx::query(
            "SELECT provider FROM provider_credentials WHERE status <> 'dead' \
             GROUP BY provider ORDER BY MIN(priority), provider",
        )
        .fetch_all(&mut *tx)
        .await
        .unwrap_or_default();
        let _ = tx.commit().await;
        rows.iter().map(|r| r.get::<String, _>("provider")).collect()
    }

    /// The explicit per-tenant STT setting (session_kv `settings:stt_provider`).
    /// `Ok(None)` = unset/"auto" (fall through to auto-derive); `Ok(Some)` = an explicit choice;
    /// `Err` = a transient DB read failure — surfaced so a blip never SILENTLY routes to the
    /// auto-derived provider when the tenant actually set a different one.
    async fn explicit_setting(&self, tenant: Uuid) -> Result<Option<String>, String> {
        let read_err = || "couldn't read your voice-transcription setting — please try again".to_string();
        let mut tx = self.pool.begin().await.map_err(|_| read_err())?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|_| read_err())?;
        let row = sqlx::query(
            "SELECT value FROM session_kv \
             WHERE tenant_id=$1 AND session_id=$2 AND key='settings:stt_provider'",
        )
        .bind(tenant)
        .bind(Uuid::nil())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| read_err())?;
        let _ = tx.commit().await;
        let Some(row) = row else { return Ok(None) };
        let v: serde_json::Value = row.get("value");
        Ok(v.get("provider")
            .and_then(|p| p.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "auto"))
    }

    /// Resolve the STT vendor: explicit setting (if STT-capable) → else the first STT-capable provider
    /// among the tenant's configured BYOK keys (priority order). `Err` carries the user-facing reason.
    async fn resolve_vendor(&self, tenant: Uuid) -> Result<&'static SttVendor, String> {
        // A DB error here is surfaced (via `?`), NOT swallowed into auto-derive — otherwise a blip
        // could silently route to a different provider than the one the tenant explicitly chose.
        if let Some(sel) = self.explicit_setting(tenant).await? {
            return stt_vendor(&sel).ok_or_else(|| {
                format!("{sel} doesn't support voice transcription — pick an STT-capable provider in Settings")
            });
        }
        for p in self.configured_providers(tenant).await {
            if let Some(v) = stt_vendor(&p) {
                return Ok(v);
            }
        }
        Err("no speech-to-text provider configured — add a key for OpenAI, Groq, Zhipu, Gemini, \
             Qwen, or MiniMax in Settings to use voice"
            .into())
    }

    /// Build the concrete `TranscriptionProvider` for `vendor`, holding the unsealed `key` for this call.
    fn build_provider(&self, vendor: &SttVendor, key: String) -> Box<dyn TranscriptionProvider> {
        match vendor.kind {
            SttKind::Multipart => {
                debug_assert!(
                    !vendor.default_model.is_empty(),
                    "multipart STT vendor {} has an empty default_model",
                    vendor.id
                );
                let model = if vendor.id == "openai" {
                    self.openai_model_override
                        .clone()
                        .unwrap_or_else(|| vendor.default_model.to_string())
                } else {
                    vendor.default_model.to_string()
                };
                Box::new(
                    OpenAiTranscriptionProvider::new(self.client.clone(), key)
                        .with_base_url(vendor.base_url)
                        .with_model(model)
                        .with_provider_name(vendor.id),
                )
            }
            SttKind::ChatAudio => {
                debug_assert!(
                    !vendor.default_model.is_empty(),
                    "chat-audio STT vendor {} has an empty default_model",
                    vendor.id
                );
                Box::new(ChatAudioTranscriptionProvider::new(
                    self.client.clone(),
                    key,
                    vendor.base_url,
                    vendor.default_model,
                    vendor.id,
                ))
            }
            SttKind::MiniMax => Box::new(MiniMaxTranscriptionProvider::new(
                self.client.clone(),
                key,
                vendor.base_url,
                vendor.default_model,
            )),
        }
    }
}

#[async_trait]
impl Transcriber for RoutedTranscriber {
    async fn transcribe(
        &self,
        tenant: Uuid,
        audio: &[u8],
        model: Option<&str>,
        language: Option<&str>,
    ) -> TranscriptionResult {
        let vendor = match self.resolve_vendor(tenant).await {
            Ok(v) => v,
            Err(reason) => {
                return TranscriptionResult {
                    success: false,
                    transcript: String::new(),
                    error: Some(reason),
                    provider: "router".into(),
                }
            }
        };
        let Some(key) = self.open_key(tenant, vendor.id).await else {
            return TranscriptionResult {
                success: false,
                transcript: String::new(),
                error: Some(format!("no usable {} key — check your key in Settings", vendor.id)),
                provider: vendor.id.into(),
            };
        };
        // The plaintext is copied into the request for this call only, then `key` drops (zeroed).
        let provider = self.build_provider(vendor, key.as_str().to_string());
        // Hermes-parity model auto-correction: a per-call model meant for ANOTHER vendor → this
        // vendor's default. Only when a per-call model is given — `None` is left as-is so the
        // provider's own default applies (incl. the QCUE_OPENAI_TRANSCRIBE_MODEL override for openai).
        let model = model.map(|m| router::stt_vendors::resolve_model(vendor, Some(m)));
        SttRouter::new(vec![provider])
            .with_constraints(vendor.constraints())
            .transcribe(audio, model, language)
            .await
    }
}
