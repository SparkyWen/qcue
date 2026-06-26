//! QCue S3-R29/R30/R32 — the Tokio worker pool over the durable `jobs` table.
//!
//! A `JobHandler` trait is the seam S2 plugs the real `ingest`/`lint`/`dream` handlers into later;
//! an `EchoHandler` no-op is registered for tests so the whole queue is exercisable keyless. Workers
//! claim via the SKIP-LOCKED query (`queue::claim_one`), run the handler, and drive the durable
//! `TaskRecord` state machine: pending → leased(running) → done(completed) | failed | (canceled).
//! Stale leases (`lease_expires < now`, a dead worker) are reclaimed back to pending — or to failed
//! once `attempt_count >= max_attempts` — by `reclaim_stale`. Worker families are gated by
//! `*_ENABLED`, so a gated-off family never runs a handler (no provider $ burned in dev; pitfall #16).
use crate::jobs::queue::{claim_one, JobKind};
use crate::tenancy::TenantTx;
use async_trait::async_trait;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// The `*_ENABLED` gate ladder. A gated-off family does no work (S3-R32).
#[derive(Clone, Copy, Debug)]
pub struct WorkerGates {
    pub ingest: bool,
    pub lint: bool,
    pub dream: bool,
    pub sync: bool,
}
impl WorkerGates {
    pub fn enabled(&self, kind: JobKind) -> bool {
        match kind {
            JobKind::Ingest | JobKind::Transcribe => self.ingest,
            JobKind::Lint => self.lint,
            JobKind::Dream => self.dream,
            JobKind::SyncMaterialize => self.sync,
            JobKind::Export => self.sync,
        }
    }
}

/// One claimed job handed to a handler.
#[derive(Clone, Debug)]
pub struct JobContext {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub kind: JobKind,
    pub payload: serde_json::Value,
    pub attempt: i32,
}

/// The seam S2 plugs `ingest`/`lint`/`dream` handlers into. The handler returns the `result` JSON to
/// persist on success, or an error string to record + retry. Handlers MUST NOT touch other tenants.
#[async_trait]
pub trait JobHandler: Send + Sync {
    async fn handle(&self, job: &JobContext) -> Result<serde_json::Value, String>;
}

/// A no-op/echo handler (tests + the keyless default): echoes the payload back as the result, runs no
/// provider call. S2 swaps in the real handlers per `kind`.
pub struct EchoHandler;
#[async_trait]
impl JobHandler for EchoHandler {
    async fn handle(&self, job: &JobContext) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({ "echo": job.payload, "kind": job.kind.as_db() }))
    }
}

/// Reclaim stale-leased jobs (a dead worker whose `lease_expires` passed) back to pending — or to
/// failed once `attempt_count >= max_attempts` (S3-R29). `attempt_count` was already bumped at claim.
pub async fn reclaim_stale(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<u64> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;
    let r = sqlx::query(
        "UPDATE jobs SET state = (CASE WHEN attempt_count >= max_attempts THEN 'failed' ELSE 'pending' END)::job_state, \
             lease_holder=NULL, lease_expires=NULL, last_error=COALESCE(last_error,'stale lease reclaimed') \
         WHERE tenant_id=$1 AND state='leased' AND lease_expires < now()",
    )
    .bind(tenant_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(r.rows_affected())
}

/// Mark a leased job completed (queued→running→completed). Records the handler `result`.
pub async fn complete(tx: &mut TenantTx, job_id: Uuid, result: serde_json::Value) -> sqlx::Result<()> {
    sqlx::query("UPDATE jobs SET state='done', result=$2, updated_at=now() WHERE id=$1")
        .bind(job_id)
        .bind(&result)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Mark a leased job failed → retry (back to pending with backoff) until `max_attempts` is reached,
/// then terminal `failed`. The lease is released either way.
pub async fn fail_or_retry(tx: &mut TenantTx, job_id: Uuid, err: &str, backoff_secs: i64) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE jobs SET state = (CASE WHEN attempt_count >= max_attempts THEN 'failed' ELSE 'pending' END)::job_state, \
             last_error=$2, available_at = now() + ($3 || ' seconds')::interval, lease_holder=NULL, lease_expires=NULL \
         WHERE id=$1",
    )
    .bind(job_id)
    .bind(err)
    .bind(backoff_secs.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Cancel a job (any non-terminal state → canceled). Used by the cancel surface / Dream rollback.
pub async fn cancel(tx: &mut TenantTx, job_id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE jobs SET state='canceled', lease_holder=NULL, lease_expires=NULL, updated_at=now() \
         WHERE id=$1 AND state IN ('pending','leased')",
    )
    .bind(job_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// A per-`JobKind` handler table. S2 registers the real `ingest` handler here; unknown kinds fall back
/// to `EchoHandler` (the keyless default). One sandbox, many prompts — the worker never branches on
/// kind beyond this lookup.
pub struct HandlerRegistry {
    ingest: Option<std::sync::Arc<dyn JobHandler>>,
    dream: Option<std::sync::Arc<dyn JobHandler>>,
    echo: EchoHandler,
}
impl Default for HandlerRegistry {
    fn default() -> Self {
        Self { ingest: None, dream: None, echo: EchoHandler }
    }
}
impl HandlerRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    /// Register the real `ingest` handler (replaces Echo for `kind='ingest'`; S2 wiring).
    pub fn with_ingest(mut self, h: std::sync::Arc<dyn JobHandler>) -> Self {
        self.ingest = Some(h);
        self
    }
    /// Register the real `dream` handler (replaces Echo for `kind='dream'`; S2 Auto-Dream wiring). The
    /// S3 dream-scheduler cron enqueues `kind='dream'` jobs; this handler runs the `DreamScheduler`.
    pub fn with_dream(mut self, h: std::sync::Arc<dyn JobHandler>) -> Self {
        self.dream = Some(h);
        self
    }
    /// Resolve the handler for a claimed job's kind; Echo is the default for unregistered kinds.
    pub fn handler_for(&self, kind: JobKind) -> &dyn JobHandler {
        match kind {
            JobKind::Ingest => self.ingest.as_deref().unwrap_or(&self.echo),
            JobKind::Dream => self.dream.as_deref().unwrap_or(&self.echo),
            _ => &self.echo,
        }
    }
}

/// One claim+dispatch tick that selects the handler per claimed job's kind from the `registry`
/// (S2 ingest wiring). Same SKIP-LOCKED lease + lifecycle-commit semantics as `run_once`.
pub async fn run_once_registry(
    pool: &PgPool,
    gates: &WorkerGates,
    tenant_id: Uuid,
    kind: JobKind,
    worker: &str,
    registry: &HandlerRegistry,
) -> sqlx::Result<u64> {
    run_once(pool, gates, tenant_id, kind, worker, registry.handler_for(kind)).await
}

/// One claim+dispatch tick for a tenant+family on `pool` using `handler`. Returns jobs processed.
/// Gated-off families do nothing (S3-R32). Each claimed job runs in its own GUC-bound tx so the
/// SKIP-LOCKED lease + the lifecycle transition commit atomically.
pub async fn run_once(
    pool: &PgPool,
    gates: &WorkerGates,
    tenant_id: Uuid,
    kind: JobKind,
    worker: &str,
    handler: &dyn JobHandler,
) -> sqlx::Result<u64> {
    if !gates.enabled(kind) {
        return Ok(0); // gate ladder: cron never burns real $ in dev (pitfall #16)
    }
    let mut processed = 0u64;
    loop {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await?;
        let claimed = match claim_one(&mut tx, tenant_id, worker).await? {
            Some(id) => id,
            None => {
                tx.rollback().await?;
                break;
            }
        };
        let row = sqlx::query("SELECT kind::text AS kind, payload, attempt_count FROM jobs WHERE id=$1")
            .bind(claimed)
            .fetch_one(&mut *tx)
            .await?;
        let job = JobContext {
            id: claimed,
            tenant_id,
            kind,
            payload: row.get::<serde_json::Value, _>("payload"),
            attempt: row.get::<i32, _>("attempt_count"),
        };
        match handler.handle(&job).await {
            Ok(result) => complete(&mut tx, claimed, result).await?,
            Err(e) => fail_or_retry(&mut tx, claimed, &e, 30).await?,
        }
        tx.commit().await?;
        processed += 1;
    }
    Ok(processed)
}
