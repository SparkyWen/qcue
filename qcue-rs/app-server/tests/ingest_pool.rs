//! QCue v0.1.1 — the ingest worker POOL orchestration (the new `main.rs` seam).
//!
//! `tests/ingest_e2e.rs` already proves the `IngestHandler` itself (idea → wiki pages). THIS test
//! proves the missing piece: `jobs::spawn::ingest_tick` SCANS active tenants (from the global
//! `tenants` table) and runs the SKIP-LOCKED claim+dispatch loop per tenant — WITHOUT the caller
//! passing a tenant id — so a captured note's enqueued `kind='ingest'` job is actually picked up.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::jobs::queue::{enqueue, JobKind};
use app_server::jobs::spawn::ingest_tick;
use app_server::jobs::worker::{HandlerRegistry, WorkerGates};
use sqlx::PgPool;

// ── gated-off pool does no work; gated-on pool drains the tenant's pending ingest job via scan ──
#[sqlx::test(migrations = "../migrations")]
async fn test_ingest_pool_scans_tenants_and_drains(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "pool-a").await;
    let mut tx = tenant_tx(&db, tid).await;
    let jid = enqueue(&mut tx, tid, Some(uid), JobKind::Ingest, serde_json::json!({"idea_id": "x"}), None)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // Echo registry (no real LLM): we are testing the POOL ORCHESTRATION, not the handler. Note the
    // tick is NOT told which tenant — it discovers the seeded tenant by scanning `tenants`.
    let registry = HandlerRegistry::new();

    // gated OFF → the scan finds the tenant but the gate makes run_once a no-op → 0 processed, still pending.
    let off = WorkerGates { ingest: false, lint: false, dream: false, sync: false };
    let n = ingest_tick(&db.app, &off, "ingest-0", &registry).await.unwrap();
    assert_eq!(n, 0, "gated-off ingest pool processes zero jobs");
    assert_eq!(job_state(&db, tid, jid).await, "pending", "gated-off leaves the job pending");

    // gated ON → the scan finds the tenant and the echo handler completes the job.
    let on = WorkerGates { ingest: true, lint: false, dream: false, sync: false };
    let n = ingest_tick(&db.app, &on, "ingest-0", &registry).await.unwrap();
    assert_eq!(n, 1, "enabled ingest pool discovers the tenant and processes its pending job");
    assert_eq!(job_state(&db, tid, jid).await, "done", "the scanned-and-claimed ingest job completes");
}

// ── two tenants, each with a pending ingest job → one tick drains BOTH (cross-tenant scan) ──────
#[sqlx::test(migrations = "../migrations")]
async fn test_ingest_pool_drains_multiple_tenants(pool: PgPool) {
    let db = from_pool(pool);
    let (t1, u1) = seed_tenant(&db, "pool-b1").await;
    let (t2, u2) = seed_tenant(&db, "pool-b2").await;
    for (t, u) in [(t1, u1), (t2, u2)] {
        let mut tx = tenant_tx(&db, t).await;
        enqueue(&mut tx, t, Some(u), JobKind::Ingest, serde_json::json!({"idea_id": "x"}), None)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    let on = WorkerGates { ingest: true, lint: false, dream: false, sync: false };
    let n = ingest_tick(&db.app, &on, "ingest-0", &HandlerRegistry::new()).await.unwrap();
    assert_eq!(n, 2, "a single tick scans and drains every tenant's pending ingest job");
}

/// Read a job's state under the tenant GUC (`jobs` is FORCE RLS).
async fn job_state(db: &TestDb, tid: uuid::Uuid, jid: uuid::Uuid) -> String {
    let mut tx = tenant_tx(db, tid).await;
    let s: String = sqlx::query_scalar("SELECT state::text FROM jobs WHERE id=$1")
        .bind(jid)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    s
}
