//! QCue S3 — shared app state injected via axum::extract::State.
use crate::config::Config;
use crate::objstore::ObjStore;
use crate::transcribe::Transcriber;
use crate::vault::secrets::Secrets;
use crate::wire::hub::StreamHub;
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use wiki::llm::WikiLlm;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub pool: PgPool,      // qcue_app
    pub auth_pool: PgPool, // qcue_auth (narrow bootstrap, S3-R4)
    pub secrets: Arc<dyn Secrets>, // BYOK seal/open seam (the plaintext key never escapes this boundary)
    pub objstore: Arc<ObjStore>,   // single-root, realpath-guarded JSONL object store
    /// Per-Thread broadcast + 20-event replay registry for the recall/wiki-query SSE streams (S3-R37/R40).
    pub threads: StreamHub,
    /// Per-job broadcast + replay registry for the Dream-detail SSE stream (`dream_started/progress/…`).
    pub dream_streams: StreamHub,
    /// The recall/wiki-query model seam — the AGENTIC one: advertises + really executes `recall_search`
    /// so the model drives its own RLS-scoped search. In tests a stub-backed harness keeps it keyless.
    pub recall_llm: Arc<dyn WikiLlm>,
    /// The ingest/extraction model seam — PLAIN (no tools): the 6-stage pipeline calls this for entity/
    /// concept extraction. Kept separate from `recall_llm` so extraction never advertises recall tools.
    pub ingest_llm: Arc<dyn WikiLlm>,
    /// The voice transcription seam (D4): stub in tests/demos, OpenAI BYOK in prod. The `/v1/transcribe`
    /// route depends only on this trait, never on stub-vs-real.
    pub transcriber: Arc<dyn Transcriber>,
    /// Process-wide Google JWKS cache for `POST /v1/auth/social` id_token verification (NG-R2).
    pub jwks: Arc<crate::auth::social::Jwks>,
}

impl AppState {
    /// The per-tenant wiki vault root `<data_root>/objects/t/<tenant>/u/_` (matches the ingest write-gate
    /// root, so the recall query engine reads the same bodies the ingest pipeline writes).
    pub fn vault_root(&self, tenant: uuid::Uuid) -> PathBuf {
        PathBuf::from(&self.cfg.data_root)
            .join("objects")
            .join(format!("t/{tenant}/u/_"))
    }
}
