//! QCue DIG-R2/DIG-R3/DIG-R7 — `POST /v1/ingest/run`: the one-click incremental digest. Scans the
//! tenant's DIRTY captures (pending OR edited-since-ingest) under the request tenant GUC (FORCE RLS),
//! enqueues one DEBOUNCED `kind='ingest'` job per dirty idea (`debounce_ref="ingest:{id}"` — the SAME
//! ref the auto-ingest capture path uses, so a manual digest collapses onto any pending auto-ingest),
//! and returns `IngestRunResult { enqueued, job_ids }`. The existing `IngestHandler`/`IngestJob::run`
//! runs the jobs unchanged (write-gate single-site + cost pre-check preserved). DIG-R7: jobs execute
//! only when `INGEST_WORKERS_ENABLED` (same dependency as auto-ingest); this endpoint never runs them
//! inline — it only enqueues. Idempotent under repeat clicks (debounce), bounded by MAX_PENDING_PER_TENANT.
use crate::error::ApiError;
use crate::jobs::queue::{enqueue, JobKind};
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use app_server_protocol::v1::IngestRunResult;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use store::ideas_repo::IdeasRepo;

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/ingest/run", post(run))
}

/// `POST /v1/ingest/run` — enqueue a debounced ingest job for each dirty capture; returns the count + ids.
async fn run(
    State(st): State<AppState>,
    mut ctx: TenantCtx,
) -> Result<Json<IngestRunResult>, ApiError> {
    // 1) the dirty scan (its own tenant-GUC tx via the repo): pending + edited-since-ingest ideas.
    let dirty = IdeasRepo::new(st.pool.clone())
        .select_dirty_for_ingest(ctx.tenant_id)
        .await?;
    // 2) enqueue one debounced ingest job per dirty idea on the REQUEST tx (commits together below).
    let mut job_ids = Vec::with_capacity(dirty.len());
    for idea_id in &dirty {
        let job_id = enqueue(
            &mut ctx.tx,
            ctx.tenant_id,
            Some(ctx.user_id),
            JobKind::Ingest,
            serde_json::json!({ "idea_id": idea_id }),
            Some(&format!("ingest:{idea_id}")),
        )
        .await
        .map_err(|_| ApiError::Overloaded)?;
        job_ids.push(job_id);
    }
    ctx.tx.commit().await?;
    let enqueued = u32::try_from(job_ids.len()).unwrap_or(u32::MAX);
    Ok(Json(IngestRunResult { enqueued, job_ids }))
}
