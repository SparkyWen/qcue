//! QCue S3-R27/R28/R31 — the durable SKIP-LOCKED job queue (Appendix B §4.15 verbatim claim query)
//! + per-tenant bound (-32001 "overloaded") + debounce.
//!
//! The queue is durable: every state transition is a row in `jobs` (no in-memory-only state, B-R40).
//! Workers claim with `SELECT … FOR UPDATE SKIP LOCKED` so N workers never grab the same row, and the
//! claim scan is tenant-scoped + per-tenant bounded so one tenant cannot starve another (RKM §6).
use crate::tenancy::TenantTx;
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

/// The `job_kind` enum (Appendix B §2.1). `as_db` yields the Postgres enum label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Ingest,
    Lint,
    Dream,
    Transcribe,
    SyncMaterialize,
    Export,
}
impl JobKind {
    pub fn as_db(&self) -> &'static str {
        match self {
            JobKind::Ingest => "ingest",
            JobKind::Lint => "lint",
            JobKind::Dream => "dream",
            JobKind::Transcribe => "transcribe",
            JobKind::SyncMaterialize => "sync_materialize",
            JobKind::Export => "export",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EnqueueError {
    /// Per-tenant in-flight budget exceeded → JSON-RPC -32001 "overloaded; retry later".
    #[error("server overloaded; retry later")]
    Overloaded,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// The per-tenant bound on pending+leased jobs (the live budget; rebuild-safe SQL `count(*)` fallback).
pub const MAX_PENDING_PER_TENANT: i64 = 500;

/// Enqueue with per-tenant bound + debounce. `debounce_ref` collapses repeats within the window:
/// a second identical enqueue for the same (kind, ref) returns the existing pending row's id.
pub async fn enqueue(
    tx: &mut TenantTx,
    tenant_id: Uuid,
    user_id: Option<Uuid>,
    kind: JobKind,
    payload: serde_json::Value,
    debounce_ref: Option<&str>,
) -> Result<Uuid, EnqueueError> {
    // bound: count pending+leased for this tenant (tenant-scoped, so it can't starve other tenants).
    let inflight: i64 =
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE tenant_id=$1 AND state IN ('pending','leased')")
            .bind(tenant_id)
            .fetch_one(&mut **tx)
            .await?;
    if inflight >= MAX_PENDING_PER_TENANT {
        return Err(EnqueueError::Overloaded);
    }
    // debounce: if an identical pending job for this ref already exists, return it (no second row).
    if let Some(reff) = debounce_ref
        && let Some(r) = sqlx::query(
            "SELECT id FROM jobs WHERE tenant_id=$1 AND kind=$2::job_kind AND state='pending' AND payload->>'debounce_ref'=$3",
        )
        .bind(tenant_id)
        .bind(kind.as_db())
        .bind(reff)
        .fetch_optional(&mut **tx)
        .await?
    {
        return Ok(r.get::<Uuid, _>("id"));
    }
    let id = Uuid::now_v7();
    let mut pl = payload;
    if let (Some(reff), Some(obj)) = (debounce_ref, pl.as_object_mut()) {
        obj.insert("debounce_ref".into(), serde_json::json!(reff));
    }
    sqlx::query("INSERT INTO jobs(id,tenant_id,user_id,kind,payload) VALUES ($1,$2,$3,$4::job_kind,$5)")
        .bind(id)
        .bind(tenant_id)
        .bind(user_id)
        .bind(kind.as_db())
        .bind(&pl)
        .execute(&mut **tx)
        .await?;
    Ok(id)
}

/// The exact Appendix B §4.15 claim query: atomically lease the highest-priority/oldest pending job
/// via `FOR UPDATE SKIP LOCKED`. Two concurrent workers never grab the same row.
pub async fn claim_one(tx: &mut TenantTx, tenant_id: Uuid, worker: &str) -> sqlx::Result<Option<Uuid>> {
    let row = sqlx::query(
        "UPDATE jobs SET state='leased', lease_holder=$2, lease_expires=now()+interval '5 minutes', \
             attempt_count=attempt_count+1, updated_at=now() \
         WHERE id = ( SELECT id FROM jobs WHERE tenant_id=$1 AND state='pending' AND available_at<=now() \
             ORDER BY priority DESC, available_at FOR UPDATE SKIP LOCKED LIMIT 1 ) RETURNING id",
    )
    .bind(tenant_id)
    .bind(worker)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|r| r.get::<Uuid, _>("id")))
}
