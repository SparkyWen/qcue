//! QCue S3-R47/R48 — device register (per-tenant `site_id`) + idempotent, totally-ordered op push,
//! and the `/v1/sync/register` · `/v1/sync/push` · `/v1/sync/pull` HTTP surface.
//!
//! `register_device` mints a small per-tenant `site_id` (the HLC tiebreaker, B §4.5) and is idempotent
//! on `(tenant, user, platform, display_name)`. `push_ops` inserts each op with `ON CONFLICT
//! (tenant,device,site,lamport) DO NOTHING`, so a re-sent op is a no-op (B-R21); the `op` JSONB is
//! redacted at the boundary so no provider key reaches `sync_ops.op` (B-R11). All reads/writes go
//! through the GUC-bound `TenantTx`, so RLS isolates `sync_ops` across tenants.
use crate::error::ApiError;
use crate::state::AppState;
use crate::sync::materialize::{apply_unapplied, ops_since_seq, snapshot, tenant_cursor};
use crate::tenancy::{TenantCtx, TenantTx};
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

// SYNC-D8: the op wire type now lives in the serde-only protocol crate (re-exported so existing
// `app_server::sync::routes::SyncOp` references — tests included — keep resolving).
pub use protocol::sync::SyncOp;
use protocol::sync::SyncDelta;
use store::wiki_repo::WikiRepo;
use wiki::sandbox::{TenantQuota, TenantSandbox};
use wiki::write_gate::WikiWriteGate;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/sync/register", post(register_route))
        .route("/v1/sync/push", post(push_route))
        .route("/v1/sync/pull", get(pull_route))
}

/// A registered device: its id + the per-tenant `site_id` HLC tiebreaker.
pub struct DeviceReg {
    pub device_id: Uuid,
    pub site_id: i64,
}

/// Create or return a device with a per-tenant-unique small `site_id` (HLC tiebreaker, B §4.5).
/// Idempotent on `(tenant, user, platform, display_name)`: a re-register returns the same row.
pub async fn register_device(
    tx: &mut TenantTx,
    tenant_id: Uuid,
    user_id: Uuid,
    platform: &str,
    name: &str,
) -> sqlx::Result<DeviceReg> {
    if let Some(r) = sqlx::query(
        "SELECT id, site_id FROM devices WHERE tenant_id=$1 AND user_id=$2 AND platform=$3 AND display_name=$4",
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(platform)
    .bind(name)
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok(DeviceReg { device_id: r.get("id"), site_id: r.get("site_id") });
    }
    let next: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(site_id),0)+1 FROM devices WHERE tenant_id=$1")
        .bind(tenant_id)
        .fetch_one(&mut **tx)
        .await?;
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO devices(id,tenant_id,user_id,platform,display_name,site_id) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(id)
        .bind(tenant_id)
        .bind(user_id)
        .bind(platform)
        .bind(name)
        .bind(next)
        .execute(&mut **tx)
        .await?;
    Ok(DeviceReg { device_id: id, site_id: next })
}

/// The push outcome: how many ops were newly inserted (a re-send conflict counts 0).
pub struct PushResult {
    pub inserted: u64,
}

/// Max CRDT ops accepted in a single `POST /v1/sync/push` (DoS bound; clients batch beyond this).
const MAX_OPS_PER_PUSH: usize = 1000;

/// Insert each op; the unique `(tenant,device,site,lamport)` makes a re-send an idempotent no-op (B-R21).
/// The `op` JSONB is redacted before INSERT so no provider key ever reaches `sync_ops.op` (B-R11).
pub async fn push_ops(
    tx: &mut TenantTx,
    tenant_id: Uuid,
    user_id: Uuid,
    device_id: Uuid,
    ops: &[SyncOp],
) -> sqlx::Result<PushResult> {
    let mut inserted = 0u64;
    for o in ops {
        let mut op_json = o.op.clone();
        crate::redact::redact_json(&mut op_json); // B-R11: never let a key reach sync_ops.op
        let r = sqlx::query(
            "INSERT INTO sync_ops(tenant_id,user_id,device_id,hlc_wall_ms,hlc_lamport,site_id,entity_kind,entity_ref,op) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (tenant_id,device_id,site_id,hlc_lamport) DO NOTHING",
        )
        .bind(tenant_id)
        .bind(user_id)
        .bind(device_id)
        .bind(o.hlc_wall_ms)
        .bind(o.hlc_lamport)
        .bind(o.site_id)
        .bind(&o.entity_kind)
        .bind(&o.entity_ref)
        .bind(&op_json)
        .execute(&mut **tx)
        .await?;
        inserted += r.rows_affected();
    }
    Ok(PushResult { inserted })
}

// ── HTTP surface ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RegisterReq {
    platform: String,
    #[serde(default)]
    display_name: String,
}
#[derive(Serialize)]
struct RegisterResp {
    device_id: Uuid,
    site_id: i64,
}

/// `POST /v1/sync/register` — register (or return) this device + its per-tenant `site_id`.
async fn register_route(
    State(_st): State<AppState>,
    mut ctx: TenantCtx,
    Json(req): Json<RegisterReq>,
) -> Result<Json<RegisterResp>, ApiError> {
    let reg = register_device(&mut ctx.tx, ctx.tenant_id, ctx.user_id, &req.platform, &req.display_name).await?;
    ctx.tx.commit().await?;
    Ok(Json(RegisterResp { device_id: reg.device_id, site_id: reg.site_id }))
}

#[derive(Deserialize)]
struct PushReq {
    device_id: Uuid,
    ops: Vec<SyncOp>,
}
#[derive(Serialize)]
struct PushResp {
    inserted: u64,
    /// The tenant's pull cursor after this push (SYNC-D4) — the pusher persists it so its own ops don't
    /// echo back on the next pull.
    cursor: i64,
}

/// `POST /v1/sync/push` — push a batch of ops (idempotent), then materialize the unapplied tail.
async fn push_route(
    State(st): State<AppState>,
    mut ctx: TenantCtx,
    Json(req): Json<PushReq>,
) -> Result<Json<PushResp>, ApiError> {
    // Bound a single push: the body cap already limits total bytes, but cap the op COUNT too so one
    // request can't pin a DB connection through an arbitrarily long sequential-INSERT loop (DoS). A
    // client with more pending ops simply pushes in batches.
    if req.ops.len() > MAX_OPS_PER_PUSH {
        return Err(ApiError::BadRequest(format!("too many ops in one push (max {MAX_OPS_PER_PUSH})")));
    }
    let res = push_ops(&mut ctx.tx, ctx.tenant_id, ctx.user_id, req.device_id, &req.ops).await?;
    // materialize the just-pushed (and any prior unapplied) ops in HLC order (conflict-free) into the
    // canonical tables. `idea.create` writes the canonical JSONL via the object store (log_ref);
    // `wiki_page` ops flow through the single write-gate (SYNC-D3), which owns its own RLS via `tenant`.
    let gate = WikiWriteGate::new(
        WikiRepo::new(st.pool.clone()),
        TenantSandbox { vault_root: st.vault_root(ctx.tenant_id), quota: TenantQuota::from_env() },
    );
    apply_unapplied(&mut ctx.tx, ctx.tenant_id, ctx.user_id, &st.objstore, &gate).await?;
    let cursor = tenant_cursor(&mut ctx.tx, ctx.tenant_id).await?;
    ctx.tx.commit().await?;
    Ok(Json(PushResp { inserted: res.inserted, cursor }))
}

/// `GET /v1/sync/pull?since=<seq>` — the read-sync change feed (SSE-allowlisted so a reconnecting device
/// can authenticate via `?token=`). A COLD pull (`since` absent/0) returns a `SyncDelta` snapshot of the
/// canonical tables (SYNC-D5); a WARM pull returns the incremental ops with `seq > since` (SYNC-D4).
/// Both carry the next `cursor` (read BEFORE the ops, so a concurrent insert re-sends rather than skips).
async fn pull_route(
    State(_st): State<AppState>,
    mut ctx: TenantCtx,
) -> Result<Json<SyncDelta>, ApiError> {
    let since: i64 = ctx.query_param("since").and_then(|s| s.parse().ok()).unwrap_or(0);
    let cursor = tenant_cursor(&mut ctx.tx, ctx.tenant_id).await?;
    let delta = if since <= 0 {
        SyncDelta {
            cursor,
            snapshot: Some(snapshot(&mut ctx.tx, ctx.tenant_id).await?),
            ops: vec![],
        }
    } else {
        SyncDelta {
            cursor,
            snapshot: None,
            ops: ops_since_seq(&mut ctx.tx, ctx.tenant_id, since).await?,
        }
    };
    ctx.tx.commit().await?;
    Ok(Json(delta))
}
