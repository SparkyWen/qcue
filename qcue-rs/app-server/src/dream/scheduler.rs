//! QCue S3-R54 — the Auto-Dream scheduler cron. A Tokio-interval tick (gated by `DREAM_ENABLED`,
//! `*_ENABLED=false` in dev so no provider $ is burned on a test DB — pitfall #16) that, per ACTIVE
//! tenant, runs the CHEAP pre-check `DreamScheduler::dream_due(tenant)` (the lock-as-clock + time +
//! scan-throttle gates only — A-R3: one indexed single-row read) and, when due, enqueues a `kind='dream'`
//! job. The existing `DreamHandler` (registered for `kind='dream'`) then runs the full gate ladder + the
//! forked agent. Per-tenant + idempotent: the `jobs` debounce-ref (`dream:<tenant>`) collapses a repeat
//! enqueue within the window, and the lock-as-clock prevents a double-fire (a second tick before the
//! first dream advanced the clock still sees `dream_due` true but the debounced enqueue is a no-op).
use crate::jobs::queue::{enqueue, JobKind};
use crate::state::AppState;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::time::Duration;
use store::ideas_repo::IdeasRepo;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wiki::dream::lock::PgConsolidationLock;
use wiki::dream::scheduler::{DreamOutcome, DreamRunner, DreamScheduler};

/// The cron tick interval. The cheap pre-check is near-zero cost, so a 60s tick is plenty responsive
/// while keeping the per-tenant scan well under the 10-min scan-throttle (the real backoff lives in the
/// lock-as-clock, A-R11).
pub const TICK: Duration = Duration::from_secs(60);

/// A no-op `DreamRunner` — the cron only ever calls `dream_due` (the cheap pre-check), which never
/// touches the runner; the REAL forked agent runs inside the `DreamHandler` once the job is claimed.
struct NoopRunner;
#[async_trait]
impl DreamRunner for NoopRunner {
    async fn run(
        &self,
        _tenant: Uuid,
        _user: Uuid,
        _since: DateTime<Utc>,
        _cancel: CancellationToken,
    ) -> anyhow::Result<DreamOutcome> {
        Ok(DreamOutcome::default())
    }
}

/// List the active tenants the cron should scan (any tenant with a consolidation row OR captures). The
/// cheap pre-check short-circuits a not-due tenant, so over-listing is harmless.
async fn active_tenants(state: &AppState) -> sqlx::Result<Vec<(Uuid, Uuid)>> {
    // (tenant_id, a representative user_id) — the dream payload keys the per-user cost ledger off it.
    // Reads `tenants`/`users` directly (the cron runs as the system, not a request); tenant scoping is
    // explicit in the per-tenant enqueue tx below.
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT DISTINCT ON (u.tenant_id) u.tenant_id, u.id \
         FROM users u JOIN tenants t ON t.id = u.tenant_id \
         WHERE t.dream_enabled ORDER BY u.tenant_id, u.created_at",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

/// One cron tick: for each active tenant, if `dream_due` is true, enqueue a debounced `kind='dream'`
/// job. Returns how many jobs were enqueued (0 when gated off or nothing is due). Per-tenant, idempotent.
pub async fn tick_once(state: &AppState) -> anyhow::Result<u64> {
    if !state.cfg.dream_enabled {
        return Ok(0); // gate ladder: the cron never enqueues real work in dev (pitfall #16)
    }
    let mut enqueued = 0u64;
    for (tenant, user) in active_tenants(state).await? {
        let sched = DreamScheduler::new(
            PgConsolidationLock::new(state.pool.clone()),
            IdeasRepo::new(state.pool.clone()),
            NoopRunner,
        );
        // the cheap pre-check (enabled + time + scan-throttle only, A-R3); a not-due tenant is a no-op.
        if sched.dream_due(tenant).await? {
            let mut tx = crate::tenancy::open_tenant_tx(&state.pool, tenant)
                .await
                .map_err(|e| anyhow::anyhow!("open tenant tx: {e:?}"))?;
            let payload = serde_json::json!({
                "user_id": user.to_string(),
                "current_session": Uuid::now_v7().to_string(),
            });
            // debounce-ref `dream:<tenant>` collapses a repeat enqueue → idempotent (the lock-as-clock
            // is the second belt: a double-fire still resolves to one running dream).
            match enqueue(&mut tx, tenant, Some(user), JobKind::Dream, payload, Some(&format!("dream:{tenant}"))) .await {
                Ok(_) => {
                    tx.commit().await?;
                    enqueued += 1;
                }
                Err(_) => {
                    // overloaded / db error → leave the clock untouched (a later tick retries).
                    let _ = tx.rollback().await;
                }
            }
        }
    }
    Ok(enqueued)
}

/// Spawn the per-tenant Dream-scheduler cron as a background task. A no-op when `dream_enabled` is false
/// (dev gate, S3-R32/pitfall #16) — it logs and returns without spawning a ticking loop.
pub fn spawn_scheduler(state: AppState) {
    if !state.cfg.dream_enabled {
        tracing::info!("dream scheduler gated off (DREAM_ENABLED=false); not spawning");
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(TICK);
        loop {
            interval.tick().await;
            match tick_once(&state).await {
                Ok(n) if n > 0 => tracing::info!(enqueued = n, "dream scheduler enqueued due jobs"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "dream scheduler tick failed"),
            }
        }
    });
}
