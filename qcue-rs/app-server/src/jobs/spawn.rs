//! QCue v0.1.1 — the ingest worker pool spawn.
//!
//! Closes the load-bearing `main.rs` gap: only the Dream scheduler was spawned, so captured notes
//! enqueued a `kind='ingest'` job that NOTHING ever claimed. This spawns `INGEST_WORKER_COUNT`
//! (default 2) Tokio worker tasks that, on a short tick, scan the active tenants and run the
//! SKIP-LOCKED claim+dispatch loop (`worker::run_once_registry`) with the real `IngestHandler`
//! registered — so a captured note is extracted into the wiki. Gated by `INGEST_WORKERS_ENABLED`
//! (pitfall #16: a gated-off pool burns no provider $).
//!
//! Tenant scan reads the RLS-free global `tenants` table (the pool runs as the system here, exactly
//! like the Dream cron's `active_tenants`); each claimed job then runs inside its own GUC-bound tx
//! (`run_once` sets `app.tenant_id` before `claim_one`), so RLS still isolates the actual work.
use crate::ingest::IngestHandler;
use crate::jobs::queue::JobKind;
use crate::jobs::worker::{reclaim_stale, run_once_registry, HandlerRegistry, JobHandler, WorkerGates};
use crate::state::AppState;
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// The worker tick. Short (the cheap scan + SKIP-LOCKED claim is near-zero when idle) so a captured
/// note is processed within a few seconds, not the Dream cron's 60s.
pub const INGEST_TICK: Duration = Duration::from_secs(5);
const DEFAULT_WORKERS: usize = 2;

/// The `*_ENABLED` gate ladder built from config (S3-R32).
pub fn gates_from(cfg: &crate::config::Config) -> WorkerGates {
    WorkerGates {
        ingest: cfg.ingest_enabled,
        lint: cfg.lint_enabled,
        dream: cfg.dream_enabled,
        sync: cfg.sync_enabled,
    }
}

/// The active tenants the ingest pool should scan. Reads the GLOBAL `tenants` table (no RLS — the
/// system root, Appendix B §4.1), so a cross-tenant scan is allowed; the per-tenant work below is
/// still RLS-bound. Over-listing is harmless: `run_once` returns 0 for a tenant with no due jobs.
async fn active_tenants(pool: &PgPool) -> sqlx::Result<Vec<Uuid>> {
    let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM tenants").fetch_all(pool).await?;
    Ok(rows.into_iter().map(|(t,)| t).collect())
}

/// One pool tick: reclaim any dead-worker leases, then run the claim+dispatch loop for every active
/// tenant. Returns the total jobs processed this tick. Factored out of `spawn_ingest_pool` so the
/// orchestration (tenant scan → per-tenant `run_once`) is testable without an infinite loop.
pub async fn ingest_tick(
    pool: &PgPool,
    gates: &WorkerGates,
    worker: &str,
    registry: &HandlerRegistry,
) -> sqlx::Result<u64> {
    let mut processed = 0u64;
    for tenant in active_tenants(pool).await? {
        // Reclaim first so a crashed worker's expired leases come back before we claim (S3-R29). The
        // 5-min lease means a still-running worker's job is never reclaimed out from under it.
        if let Err(e) = reclaim_stale(pool, tenant).await {
            tracing::warn!(error = %e, %tenant, "ingest pool: reclaim_stale failed");
        }
        processed += run_once_registry(pool, gates, tenant, JobKind::Ingest, worker, registry).await?;
    }
    Ok(processed)
}

/// Spawn the ingest worker pool as `N` background Tokio tasks. A no-op (logs + returns) when
/// `ingest_enabled` is false (dev gate, S3-R32/pitfall #16). The real `IngestHandler` is built once
/// and shared (read-only) across the workers; SKIP-LOCKED ensures no job is processed twice.
pub fn spawn_ingest_pool(state: AppState) {
    if !state.cfg.ingest_enabled {
        tracing::info!("ingest worker pool gated off (INGEST_WORKERS_ENABLED=false); not spawning");
        return;
    }
    let workers = std::env::var("INGEST_WORKER_COUNT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_WORKERS);

    // The ingest write-gate roots bodies under `<data_root>/objects/t/<tenant>/u/_` — the SAME root
    // `AppState::vault_root` (recall) reads from, so what ingest writes, recall can later read.
    let vault_base = PathBuf::from(&state.cfg.data_root).join("objects");
    // Extraction uses the PLAIN ingest_llm (no recall tools advertised) — never the agentic recall_llm.
    let handler: Arc<dyn JobHandler> =
        Arc::new(IngestHandler::new(state.pool.clone(), vault_base, state.ingest_llm.clone()));
    let registry = Arc::new(HandlerRegistry::new().with_ingest(handler));
    let gates = gates_from(&state.cfg);

    for i in 0..workers {
        let pool = state.pool.clone();
        let registry = registry.clone();
        let worker_id = format!("ingest-{i}");
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(INGEST_TICK);
            loop {
                interval.tick().await;
                match ingest_tick(&pool, &gates, &worker_id, &registry).await {
                    Ok(n) if n > 0 => tracing::info!(processed = n, worker = %worker_id, "ingest pool processed jobs"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, worker = %worker_id, "ingest pool tick failed"),
                }
            }
        });
    }
    tracing::info!(workers, "ingest worker pool spawned");
}
