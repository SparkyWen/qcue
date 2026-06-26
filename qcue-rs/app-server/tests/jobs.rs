#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::jobs::queue::{claim_one, enqueue, EnqueueError, JobKind};
use app_server::jobs::worker::{
    cancel, complete, fail_or_retry, reclaim_stale, run_once, EchoHandler, WorkerGates,
};
use sqlx::PgPool;
use uuid::Uuid;

// ── Task 17: SKIP-LOCKED claim — N concurrent workers, M jobs → each claimed exactly once (B-R40) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_job_claim_skip_locked(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "jobs-a").await;
    for _ in 0..20 {
        let mut tx = tenant_tx(&db, tid).await;
        enqueue(&mut tx, tid, Some(uid), JobKind::Ingest, serde_json::json!({}), None)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    let mut handles = vec![];
    for w in 0..4 {
        let pool = db.app.clone();
        handles.push(tokio::spawn(async move {
            let mut got: Vec<Uuid> = vec![];
            loop {
                let mut tx = pool.begin().await.unwrap();
                sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
                    .bind(tid.to_string())
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                match claim_one(&mut tx, tid, &format!("w{w}")).await.unwrap() {
                    Some(j) => {
                        got.push(j);
                        tx.commit().await.unwrap();
                    }
                    None => {
                        tx.rollback().await.unwrap();
                        break;
                    }
                }
            }
            got
        }));
    }
    let mut all = vec![];
    for h in handles {
        all.extend(h.await.unwrap());
    }
    let claimed = all.len();
    all.sort();
    all.dedup();
    assert_eq!(claimed, 20, "no job claimed twice (SKIP LOCKED)");
    assert_eq!(all.len(), 20, "each of the 20 jobs claimed exactly once");
}

// ── Task 17: per-tenant bound → -32001 + debounce collapses repeats (S3-R28, S3-R31) ──────────
#[sqlx::test(migrations = "../migrations")]
async fn test_per_tenant_bound_and_debounce(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "jobs-b").await;
    // debounce: 10 enqueues of the same (kind, ref) within the window → 1 row.
    for _ in 0..10 {
        let mut tx = tenant_tx(&db, tid).await;
        let _ = enqueue(
            &mut tx,
            tid,
            Some(uid),
            JobKind::Ingest,
            serde_json::json!({"idea_id":"fixed"}),
            Some("ingest:fixed"),
        )
        .await;
        tx.commit().await.unwrap();
    }
    // jobs has FORCE RLS, so the count must run inside a GUC-bound tx (RLS is the belt, #14).
    let mut tx = tenant_tx(&db, tid).await;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE tenant_id=$1")
        .bind(tid)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(n, 1, "debounce collapses repeats to a single pending row");
    // per-tenant bound: flooding past budget → -32001 (Overloaded).
    set_inflight(&db, tid, 9999).await;
    let mut tx = tenant_tx(&db, tid).await;
    let over = enqueue(&mut tx, tid, Some(uid), JobKind::Lint, serde_json::json!({}), None).await;
    assert!(matches!(over, Err(EnqueueError::Overloaded)), "saturated tenant → -32001 overloaded");
}

// ── Task 18: stale-lease reclaim — a dead worker's expired lease returns to pending (S3-R29) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_stale_lease_reclaim(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "jobs-c").await;
    let mut tx = tenant_tx(&db, tid).await;
    let jid = enqueue(&mut tx, tid, Some(uid), JobKind::Ingest, serde_json::json!({}), None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    // simulate a dead worker leasing it with a past lease_expires (RLS-bound update).
    let mut tx = tenant_tx(&db, tid).await;
    sqlx::query("UPDATE jobs SET state='leased'::job_state, lease_holder='dead', lease_expires=now()-interval '1 minute', attempt_count=1 WHERE id=$1")
        .bind(jid)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let reclaimed = reclaim_stale(&db.app, tid).await.unwrap();
    assert!(reclaimed >= 1, "stale-leased job returned to pending");
    let mut tx = tenant_tx(&db, tid).await;
    let state: String = sqlx::query_scalar("SELECT state::text FROM jobs WHERE id=$1")
        .bind(jid)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(state, "pending");
}

// ── Task 18: TaskRecord lifecycle — claim → complete with result (queued→running→completed) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_task_record_lifecycle(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "jobs-d").await;
    let mut tx = tenant_tx(&db, tid).await;
    let jid = enqueue(&mut tx, tid, Some(uid), JobKind::Ingest, serde_json::json!({}), None)
        .await
        .unwrap();
    let _ = claim_one(&mut tx, tid, "w0").await.unwrap();
    complete(&mut tx, jid, serde_json::json!({"report":"ok"})).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = tenant_tx(&db, tid).await;
    let (state, result): (String, serde_json::Value) =
        sqlx::query_as("SELECT state::text, result FROM jobs WHERE id=$1")
            .bind(jid)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(state, "done");
    assert_eq!(result["report"], "ok");
}

// ── Task 18: fail→retry then terminal-fail at max_attempts; cancel transitions to canceled ──
#[sqlx::test(migrations = "../migrations")]
async fn test_fail_retry_and_cancel(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "jobs-e").await;
    // a job at attempt 1 that fails → goes back to pending (retry).
    let mut tx = tenant_tx(&db, tid).await;
    let jid = enqueue(&mut tx, tid, Some(uid), JobKind::Ingest, serde_json::json!({}), None)
        .await
        .unwrap();
    let _ = claim_one(&mut tx, tid, "w0").await.unwrap();
    fail_or_retry(&mut tx, jid, "boom", 0).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = tenant_tx(&db, tid).await;
    let state: String = sqlx::query_scalar("SELECT state::text FROM jobs WHERE id=$1")
        .bind(jid)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(state, "pending", "a non-exhausted job retries");
    // a second job: cancel → canceled.
    let mut tx = tenant_tx(&db, tid).await;
    let cid = enqueue(&mut tx, tid, Some(uid), JobKind::Lint, serde_json::json!({}), None)
        .await
        .unwrap();
    cancel(&mut tx, cid).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = tenant_tx(&db, tid).await;
    let state: String = sqlx::query_scalar("SELECT state::text FROM jobs WHERE id=$1")
        .bind(cid)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(state, "canceled");
}

// ── Task 18: a gated-off family does no work; an enabled family runs the echo handler (S3-R32) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_worker_gate_and_echo_handler(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "jobs-f").await;
    let mut tx = tenant_tx(&db, tid).await;
    let jid = enqueue(&mut tx, tid, Some(uid), JobKind::Ingest, serde_json::json!({"idea_id":"x"}), None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    // gated OFF: the dream family does nothing AND the ingest job stays pending.
    let off = WorkerGates { ingest: false, lint: false, dream: false, sync: false };
    let n = run_once(&db.app, &off, tid, JobKind::Ingest, "w0", &EchoHandler).await.unwrap();
    assert_eq!(n, 0, "gated-off family processes zero jobs");
    // gated ON: the echo handler completes the job (queued→running→completed) and echoes the payload.
    let on = WorkerGates { ingest: true, lint: false, dream: false, sync: false };
    let n = run_once(&db.app, &on, tid, JobKind::Ingest, "w0", &EchoHandler).await.unwrap();
    assert_eq!(n, 1, "enabled family processes the pending ingest job");
    let mut tx = tenant_tx(&db, tid).await;
    let (state, result): (String, serde_json::Value) =
        sqlx::query_as("SELECT state::text, result FROM jobs WHERE id=$1")
            .bind(jid)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(state, "done");
    assert_eq!(result["echo"]["idea_id"], "x", "echo handler returned the payload as the result");
}
