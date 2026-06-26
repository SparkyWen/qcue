//! QCue SYNC-D4 / Task 6 — emit a SERVER-ORIGIN sync op so a server-side change (a normal capture, and
//! later ingest/dream wiki writes) propagates to other devices on their next INCREMENTAL pull.
//!
//! Why this is needed: the app re-snapshots only on a COLD (cursor 0) pull; after that it pulls
//! incrementally by `seq`. A server-side change that never became a `sync_ops` row would therefore
//! never reach an already-warm device. These ops carry `site_id = 0` (the reserved server site — device
//! registrations start at 1, B §4.5) and are inserted `applied = true`: the canonical row already
//! exists, so they are a propagation feed, NOT work for the materializer.
use crate::tenancy::TenantTx;
use uuid::Uuid;

/// Ensure (idempotently) the per-tenant "server" device row exists and return its id. Server ops need a
/// real `devices` row (`sync_ops.device_id` is a NOT NULL FK). It claims the reserved `site_id = 0`,
/// which is unique per tenant (`devices UNIQUE(tenant_id, site_id)`) and never handed to a real device,
/// so a second call no-ops on the conflict and returns the same row.
pub async fn server_device(tx: &mut TenantTx, tenant_id: Uuid, user_id: Uuid) -> sqlx::Result<Uuid> {
    sqlx::query(
        "INSERT INTO devices(tenant_id, user_id, platform, display_name, site_id) \
         VALUES ($1, $2, 'server', 'server', 0) ON CONFLICT (tenant_id, site_id) DO NOTHING",
    )
    .bind(tenant_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query_scalar("SELECT id FROM devices WHERE tenant_id=$1 AND site_id=0")
        .bind(tenant_id)
        .fetch_one(&mut **tx)
        .await
}

/// Emit one server-origin op (`site_id = 0`, `applied = true`) into `sync_ops`, redacted (B-R11). The
/// per-tenant lamport is `MAX(hlc_lamport)+1`; a per-tenant advisory **xact** lock (released at commit)
/// serializes concurrent emits so two captures can't read the same MAX and silently collide on the
/// `(tenant, device, site, lamport)` idempotency index (which would `DO NOTHING`-drop the second op).
pub async fn emit_op(
    tx: &mut TenantTx,
    tenant_id: Uuid,
    user_id: Uuid,
    entity_kind: &str,
    entity_ref: &str,
    op: serde_json::Value,
) -> sqlx::Result<()> {
    let device_id = server_device(tx, tenant_id, user_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))")
        .bind(tenant_id)
        .execute(&mut **tx)
        .await?;
    let lamport: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(hlc_lamport),0)+1 FROM sync_ops WHERE tenant_id=$1")
            .bind(tenant_id)
            .fetch_one(&mut **tx)
            .await?;
    let wall_ms = chrono::Utc::now().timestamp_millis();
    let mut op_json = op;
    crate::redact::redact_json(&mut op_json); // B-R11: never let a key reach sync_ops.op
    sqlx::query(
        "INSERT INTO sync_ops(tenant_id,user_id,device_id,hlc_wall_ms,hlc_lamport,site_id,entity_kind,entity_ref,op,applied) \
         VALUES ($1,$2,$3,$4,$5,0,$6,$7,$8,true) ON CONFLICT (tenant_id,device_id,site_id,hlc_lamport) DO NOTHING",
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(device_id)
    .bind(wall_ms)
    .bind(lamport)
    .bind(entity_kind)
    .bind(entity_ref)
    .bind(&op_json)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
