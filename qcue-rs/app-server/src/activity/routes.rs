//! QCue S3 — the Activity READ + decision surface the Flutter client consumes (the contract in
//! `qcue_app/lib/core/net/http_api_client.dart`):
//!   - `GET  /v1/approvals`        → `{approvals:[Approval]}` pending list (the D13 gate)
//!   - `POST /v1/approvals/{id}`   `{approve:bool}` → finalize/reverse the candidate (D13, reversible)
//!   - `GET  /v1/jobs`             → `{jobs:[JobRow]}` recent jobs (newest first)
//!   - `GET  /v1/cost/today`       → `{cost_micros:int}` today's tenant spend
//!   - `GET  /v1/cost/ledger`      → `{rows:[CostLedgerRow]}` recent per-day rows (all 5 token kinds)
//!
//! Every read is tenant-scoped by FORCE RLS via the per-request `app.tenant_id` GUC (an app-level WHERE
//! is belt-and-braces only — the GUC is the real isolation, #14).
use crate::error::ApiError;
use crate::redact::redact_json;
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/approvals", get(list_approvals))
        .route("/v1/approvals/{id}", post(respond_approval))
        .route("/v1/jobs", get(list_jobs))
        .route("/v1/cost/today", get(cost_today))
        .route("/v1/cost/ledger", get(cost_ledger))
}

fn redacted(mut v: serde_json::Value) -> Json<serde_json::Value> {
    redact_json(&mut v);
    Json(v)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RespondReq {
    approve: bool,
}

/// `GET /v1/approvals` — the pending candidates (`status='pending'`, the D13 human-in-the-loop gate).
/// Maps to the Dart `Approval` shape (id/action/status/requested_by/subject_ref).
async fn list_approvals(mut ctx: TenantCtx) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, action, status::text AS status, requested_by, subject_ref \
         FROM approvals WHERE status='pending' ORDER BY created_at DESC LIMIT 500",
    )
    .fetch_all(&mut *ctx.tx)
    .await?;
    let approvals: Vec<_> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "action": r.get::<String, _>("action"),
                "status": r.get::<String, _>("status"),
                "requested_by": r.get::<String, _>("requested_by"),
                "subject_ref": r.get::<serde_json::Value, _>("subject_ref"),
            })
        })
        .collect();
    ctx.tx.commit().await?;
    Ok(redacted(serde_json::json!({ "approvals": approvals })))
}

/// `POST /v1/approvals/{id}` `{approve}` — the confirm endpoint that promotes (or reverses) a pending
/// candidate (D13). The destructive side was already applied REVERSIBLY at propose time (the affected
/// page is soft-deleted by `wiki::approvals::route_destructive`):
///
/// - approve → finalize: `status='approved'` (the soft-delete stands; the merge/delete is now canonical).
/// - reject  → reverse: `status='rejected'` AND restore (`deleted_at=NULL`) the affected page(s), so the
///   destructive op is fully undone (pitfall #18 reversibility).
///
/// Only a `pending` row may be decided (idempotent / fail-closed): a non-pending id is `not found`.
async fn respond_approval(
    State(st): State<AppState>,
    mut ctx: TenantCtx,
    Path(id): Path<Uuid>,
    Json(req): Json<RespondReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let new_status = if req.approve { "approved" } else { "rejected" };
    let row = sqlx::query(
        "UPDATE approvals SET status=$2::approval_status, decided_by=$3, decided_at=now() \
         WHERE id=$1 AND status='pending' RETURNING action, subject_ref",
    )
    .bind(id)
    .bind(new_status)
    .bind(ctx.user_id)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    let Some(r) = row else {
        ctx.tx.commit().await?;
        return Err(ApiError::NotFound);
    };
    // On reject, reverse the reversible destructive side: un-soft-delete the affected page(s). The
    // subject_ref carries the page ids the gate soft-deleted at propose time (`{from,into}` for a merge,
    // `{page}` for a delete). The merge's soft-deleted page is `from`; a delete's is `page`.
    if !req.approve {
        let subject: serde_json::Value = r.get("subject_ref");
        let action: String = r.get("action");
        let target = match action.as_str() {
            "wiki_merge" => subject.get("from"),
            "wiki_delete" => subject.get("page"),
            _ => None,
        }
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());
        if let Some(pid) = target {
            sqlx::query("UPDATE wiki_pages SET deleted_at=NULL WHERE id=$1 AND deleted_at IS NOT NULL")
                .bind(pid)
                .execute(&mut *ctx.tx)
                .await?;
        }
    }
    ctx.tx.commit().await?;
    let _ = &st; // (state reserved for a future audit hook; the decision is the canonical record here)
    Ok(redacted(serde_json::json!({ "id": id, "status": new_status })))
}

/// `GET /v1/jobs` — recent jobs for the tenant (newest first), mapped to the Dart `JobRow` shape
/// (id/kind/state/progress?/last_error?). `progress` is read from `result->>'progress'` when a leased
/// job has reported one; otherwise it is omitted (the Dart model treats it as nullable).
async fn list_jobs(mut ctx: TenantCtx) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, kind::text AS kind, state::text AS state, last_error, \
                (result->>'progress')::float8 AS progress \
         FROM jobs ORDER BY created_at DESC LIMIT 50",
    )
    .fetch_all(&mut *ctx.tx)
    .await?;
    let jobs: Vec<_> = rows
        .iter()
        .map(|r| {
            let mut o = serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "kind": r.get::<String, _>("kind"),
                "state": r.get::<String, _>("state"),
            });
            if let Some(p) = r.get::<Option<f64>, _>("progress") {
                o["progress"] = serde_json::json!(p);
            }
            if let Some(e) = r.get::<Option<String>, _>("last_error") {
                o["last_error"] = serde_json::json!(e);
            }
            o
        })
        .collect();
    ctx.tx.commit().await?;
    Ok(redacted(serde_json::json!({ "jobs": jobs })))
}

/// `GET /v1/cost/today` — today's TENANT-scope accrued spend in micros (the UI never sums usage). A
/// fresh day with no ledger row reads as 0.
async fn cost_today(mut ctx: TenantCtx) -> Result<Json<serde_json::Value>, ApiError> {
    let micros: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(cost_micros),0)::bigint FROM cost_ledger \
         WHERE scope='tenant' AND user_id IS NULL AND day=current_date",
    )
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.tx.commit().await?;
    Ok(Json(serde_json::json!({ "cost_micros": micros })))
}

/// `GET /v1/cost/ledger` — recent per-day TENANT-scope rows (all 5 CanonicalUsage token kinds incl.
/// reasoning_tokens + cost_micros), newest first. Maps to the Dart `CostLedgerRow` shape.
async fn cost_ledger(mut ctx: TenantCtx) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT day, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
                reasoning_tokens, cost_micros \
         FROM cost_ledger WHERE scope='tenant' AND user_id IS NULL ORDER BY day DESC LIMIT 90",
    )
    .fetch_all(&mut *ctx.tx)
    .await?;
    let out: Vec<_> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "day": r.get::<chrono::NaiveDate, _>("day").format("%Y-%m-%dT00:00:00Z").to_string(),
                "input_tokens": r.get::<i64, _>("input_tokens"),
                "output_tokens": r.get::<i64, _>("output_tokens"),
                "cache_read_tokens": r.get::<i64, _>("cache_read_tokens"),
                "cache_write_tokens": r.get::<i64, _>("cache_write_tokens"),
                "reasoning_tokens": r.get::<i64, _>("reasoning_tokens"),
                "cost_micros": r.get::<i64, _>("cost_micros"),
            })
        })
        .collect();
    ctx.tx.commit().await?;
    Ok(Json(serde_json::json!({ "rows": out })))
}
