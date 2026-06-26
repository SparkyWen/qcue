// QCue S2 — end-to-end Auto-Dream JobHandler wiring. Enqueue a `kind='dream'` job carrying
// `{user_id, current_session}`, register the real `DreamHandler` (backed by a stub-harness `WikiLlm` —
// keyless, networkless), run the worker once, and assert the dream fired through the gate ladder + the
// lock-as-clock: the job is `done`, its result is the "Improved N pages" report, and the clock advanced
// (so the time gate now blocks a re-run). A gated-out tenant (no captures) returns `{dreamed:false}`
// with zero provider work.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::dream::DreamHandler;
use app_server::jobs::queue::{enqueue, JobKind};
use app_server::jobs::worker::{run_once_registry, HandlerRegistry, JobHandler, WorkerGates};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// Seed the consolidation row with the clock 48h in the past (time gate open).
async fn seed_consolidation_open(db: &TestDb, tid: Uuid) {
    let mut tx = tenant_tx(db, tid).await;
    sqlx::query(
        "INSERT INTO wiki_consolidation (tenant_id, last_consolidated_at) \
         VALUES ($1, now() - interval '48 hours') ON CONFLICT DO NOTHING",
    )
    .bind(tid)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// Insert `n` captures so the session gate (minSessions=5) can pass.
async fn seed_captures(db: &TestDb, tid: Uuid, uid: Uuid, n: usize) {
    let mut tx = tenant_tx(db, tid).await;
    for _ in 0..n {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO ideas (id, tenant_id, user_id, kind, body, log_ref, origin, ingest_state) \
             VALUES ($1,$2,$3,'text','capture body',$4,'capture','pending')",
        )
        .bind(id)
        .bind(tid)
        .bind(uid)
        .bind(format!("captures/{id}.jsonl"))
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}

async fn read_clock(db: &TestDb, tid: Uuid) -> DateTime<Utc> {
    let mut tx = tenant_tx(db, tid).await;
    let r: (Option<DateTime<Utc>>,) =
        sqlx::query_as("SELECT last_consolidated_at FROM wiki_consolidation WHERE tenant_id=$1")
            .bind(tid)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    r.0.unwrap()
}

async fn enqueue_dream(db: &TestDb, tid: Uuid, uid: Uuid) {
    let mut tx = tenant_tx(db, tid).await;
    let _ = enqueue(
        &mut tx,
        tid,
        Some(uid),
        JobKind::Dream,
        serde_json::json!({ "user_id": uid.to_string(), "current_session": Uuid::now_v7().to_string() }),
        Some(&format!("dream:{tid}")),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

fn registry(db: &TestDb) -> (HandlerRegistry, tempfile::TempDir) {
    let vault = tempfile::tempdir().unwrap();
    let handler: Arc<dyn JobHandler> = Arc::new(DreamHandler::with_stub_harness(
        db.app.clone(),
        vault.path().to_path_buf(),
        "Consolidated 0 pages; nothing changed.",
    ));
    (HandlerRegistry::new().with_dream(handler), vault)
}

#[sqlx::test(migrations = "../migrations")]
async fn dream_job_fires_through_gate_ladder_and_advances_clock(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "dream-e2e").await;
    seed_consolidation_open(&db, tid).await;
    seed_captures(&db, tid, uid, 5).await; // session gate passes
    enqueue_dream(&db, tid, uid).await;

    let (registry, _vault) = registry(&db);
    let gates = WorkerGates { ingest: false, lint: false, dream: true, sync: false };
    let n = run_once_registry(&db.app, &gates, tid, JobKind::Dream, "w0", &registry)
        .await
        .unwrap();
    assert_eq!(n, 1, "the dream job was claimed and run");

    // the job is done and its result is the "Improved N pages" report.
    let (state, result): (String, serde_json::Value) = {
        let mut tx = tenant_tx(&db, tid).await;
        let r = sqlx::query_as("SELECT state::text, result FROM jobs WHERE tenant_id=$1 AND kind='dream'")
            .bind(tid)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        r
    };
    assert_eq!(state, "done");
    assert_eq!(result.get("dreamed").and_then(|v| v.as_bool()), Some(true), "result: {result}");
    assert!(result.get("improved_pages").is_some(), "the improved-pages report: {result}");

    // the lock-as-clock advanced (success keeps the clock at ~now), so a re-run is now time-gated.
    let after = read_clock(&db, tid).await;
    assert!((Utc::now() - after).num_hours() < 1, "clock advanced to ~now");
}

#[sqlx::test(migrations = "../migrations")]
async fn dream_job_gated_out_returns_no_op(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "dream-noop").await;
    seed_consolidation_open(&db, tid).await;
    // NO captures → the session gate stops the ladder (a no-op tick).
    enqueue_dream(&db, tid, uid).await;

    let (registry, _vault) = registry(&db);
    let gates = WorkerGates { ingest: false, lint: false, dream: true, sync: false };
    let n = run_once_registry(&db.app, &gates, tid, JobKind::Dream, "w0", &registry)
        .await
        .unwrap();
    assert_eq!(n, 1);

    let result: serde_json::Value = {
        let mut tx = tenant_tx(&db, tid).await;
        let r: (serde_json::Value,) =
            sqlx::query_as("SELECT result FROM jobs WHERE tenant_id=$1 AND kind='dream'")
                .bind(tid)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        tx.commit().await.unwrap();
        r.0
    };
    assert_eq!(result.get("dreamed").and_then(|v| v.as_bool()), Some(false), "gated out: {result}");
}
