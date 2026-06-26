//! QCue D4 — `POST /v1/transcribe`. Auth'd via `TenantCtx`; decodes base64 audio and hands it to the
//! per-tenant `Transcriber` seam. Carries its OWN larger body limit (audio dwarfs the global 256 KB).
use crate::error::ApiError;
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use axum::extract::{DefaultBodyLimit, State};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine;
use serde::Deserialize;

/// ~25 MB of audio + base64 overhead (≈4/3). Matches the router's `AudioConstraints::max_bytes`.
const MAX_TRANSCRIBE_BODY_BYTES: usize = 35 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/v1/transcribe",
        post(transcribe).layer(DefaultBodyLimit::max(MAX_TRANSCRIBE_BODY_BYTES)),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TranscribeReq {
    audio_b64: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

async fn transcribe(
    State(st): State<AppState>,
    ctx: TenantCtx,
    Json(req): Json<TranscribeReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tenant = ctx.tenant_id;
    let user = ctx.user_id;
    // Release the request's DB connection before the (potentially slow) STT call — the transcriber
    // opens its own short tx to load the key.
    ctx.tx.commit().await?;

    // D17/B-R20 — enforce the daily cost ceiling before the billable STT provider call (a transcription
    // spends the tenant's BYOK OpenAI key just like a chat turn). DB hiccup → allow (don't block a cheap
    // call on a transient ledger read error); over-ceiling → terminal CostCap refusal.
    match store::cost_repo::CostRepo::new(st.pool.clone()).check_ceiling(tenant, user).await {
        Ok(Ok(())) => {}
        Ok(Err(reason)) => {
            tracing::info!(%tenant, reason, "transcribe refused: daily cost ceiling reached");
            return Err(ApiError::CostCap);
        }
        Err(e) => tracing::warn!(error = %e, %tenant, "transcribe cost-ceiling check failed (allowing)"),
    }

    let audio = base64::engine::general_purpose::STANDARD
        .decode(req.audio_b64.as_bytes())
        .map_err(|_| ApiError::BadRequest("audio_b64 is not valid base64".into()))?;
    if audio.is_empty() {
        return Err(ApiError::BadRequest("empty audio".into()));
    }

    // Redaction-safe diagnostics (D4): byte length + sniffed container + the leading CONTAINER-HEADER
    // bytes only (e.g. `....ftypM4A `) — never audio content or secrets. Lets us tell a too-short /
    // non-finalized clip (tiny len) from a wrong-container upload, straight from the journal.
    let fmt = router::stt_openai::detect_audio_format(&audio);
    tracing::info!(
        audio_len = audio.len(),
        detected = fmt.kind,
        head_hex = %router::stt_openai::audio_head_hex(&audio, 16),
        "transcribe: received audio clip"
    );

    let result =
        st.transcriber.transcribe(tenant, &audio, req.model.as_deref(), req.language.as_deref()).await;
    Ok(Json(serde_json::json!({
        "transcript": result.transcript,
        "provider": result.provider,
        "success": result.success,
        "error": result.error,
    })))
}
