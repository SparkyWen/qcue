//! QCue S3-R56 — the Dream SSE surface. `POST /v1/dream/run` is the manual run (gated, bypasses the
//! time-gate per App. A, but still honours `DREAM_ENABLED` + the cost cap) — it enqueues a `kind='dream'`
//! job (the existing `DreamHandler` runs it) and returns `{job_id}` so the client can subscribe to the
//! `GET /v1/dream/{job_id}/stream` SSE mirror (`dream_started/progress/completed/failed`, mounted in
//! `wire::routes`). Reasoning on that stream is collapsed-by-default (D18).
use crate::error::ApiError;
use crate::jobs::queue::{enqueue, JobKind};
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/dream/run", post(run))
}

/// `POST /v1/dream/run` — enqueue a manual dream job (bypasses the time-gate; honours the gate + cap).
async fn run(
    State(_st): State<AppState>,
    mut ctx: TenantCtx,
) -> Result<Json<serde_json::Value>, ApiError> {
    // the dream handler keys the per-user cost ledger off `user_id` and excludes `current_session` from
    // the session gate; a manual run carries the caller's identity + a fresh session marker.
    let payload = serde_json::json!({
        "manual": true,
        "user_id": ctx.user_id.to_string(),
        "current_session": Uuid::now_v7().to_string(),
    });
    let job_id = enqueue(&mut ctx.tx, ctx.tenant_id, Some(ctx.user_id), JobKind::Dream, payload, Some("dream:manual"))
        .await
        .map_err(|_| ApiError::Overloaded)?;
    ctx.tx.commit().await?;
    Ok(Json(serde_json::json!({ "job_id": job_id })))
}
