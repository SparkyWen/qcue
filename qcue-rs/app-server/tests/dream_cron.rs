// QCue S3-R54 — the Auto-Dream scheduler cron: a per-tenant tick that, gated by DREAM_ENABLED, calls
// `DreamScheduler::dream_due(tenant)` and enqueues a `kind='dream'` job when due — per-tenant + idempotent
// (the jobs debounce-ref + the lock-as-clock prevent a double-fire). Against real Postgres (`#[sqlx::test]`).
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::config::Config;
use app_server::dream::scheduler::tick_once;
use app_server::state::AppState;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// Build an AppState whose `dream_enabled` gate is ON (the cron only enqueues when its gate is open).
fn state_dream_on(db: &TestDb) -> AppState {
    let mut raw = Config::test_raw();
    raw.dream_enabled = true;
    let cfg = Config::validate(raw).expect("valid config");
    AppState {
        cfg: Arc::new(cfg),
        pool: db.app.clone(),
        auth_pool: db.auth.clone(),
        secrets: stub_secrets(),
        objstore: Arc::new(app_server::objstore::ObjStore::new(&test_data_root())),
        threads: app_server::wire::hub::StreamHub::new(),
        dream_streams: app_server::wire::hub::StreamHub::new(),
        recall_llm: Arc::new(app_server::ingest::RouterWikiLlm::stub("ok")),
        ingest_llm: Arc::new(app_server::ingest::RouterWikiLlm::stub("ok")),
        transcriber: Arc::new(app_server::transcribe::StubTranscriber::new("stub")),
        jwks: Arc::new(app_server::auth::social::Jwks::new()),
    }
}

/// Seed the consolidation clock 48h in the past so the time-gate is open.
async fn seed_clock_open(db: &TestDb, tid: Uuid) {
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

async fn pending_dream_jobs(db: &TestDb, tid: Uuid) -> i64 {
    let mut tx = tenant_tx(db, tid).await;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE tenant_id=$1 AND kind='dream' AND state='pending'",
    )
    .bind(tid)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    n
}

// ── (d) the cron enqueues a kind='dream' job when dream_due is true, and is idempotent ─────────
#[sqlx::test(migrations = "../migrations")]
async fn test_dream_cron_enqueues_when_due_and_is_idempotent(pool: PgPool) {
    // the wiki dream_due reads DREAM_ENABLED from the env (unset → enabled). Pin it on for this process.
    unsafe { std::env::set_var("DREAM_ENABLED", "true") };
    let db = from_pool(pool);
    let (tid, _uid) = seed_tenant(&db, "cron-due").await;
    seed_clock_open(&db, tid).await; // time-gate open → dream_due true

    let st = state_dream_on(&db);
    // first tick: due → enqueues exactly one dream job.
    let n = tick_once(&st).await.unwrap();
    assert_eq!(n, 1, "the cron enqueues a dream job when dream_due is true");
    assert_eq!(pending_dream_jobs(&db, tid).await, 1);

    // second tick (before the clock advanced): the debounce-ref collapses the repeat → still ONE job.
    let n2 = tick_once(&st).await.unwrap();
    let total = pending_dream_jobs(&db, tid).await;
    assert_eq!(total, 1, "idempotent: a second tick does not double-fire (debounce + lock-as-clock)");
    let _ = n2; // n2 may be 0 (debounced) — the invariant is the single pending job above.
}

// ── a not-due tenant (clock just ran) is a no-op: the cheap time-gate stops the cron ──────────
#[sqlx::test(migrations = "../migrations")]
async fn test_dream_cron_skips_when_not_due(pool: PgPool) {
    unsafe { std::env::set_var("DREAM_ENABLED", "true") };
    let db = from_pool(pool);
    let (tid, _uid) = seed_tenant(&db, "cron-soon").await;
    // clock = now() → the 24h time-gate blocks; dream_due is false.
    let mut tx = tenant_tx(&db, tid).await;
    sqlx::query("INSERT INTO wiki_consolidation (tenant_id, last_consolidated_at) VALUES ($1, now())")
        .bind(tid)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let st = state_dream_on(&db);
    let n = tick_once(&st).await.unwrap();
    assert_eq!(n, 0, "a not-due tenant enqueues nothing (cheapest-gate-first)");
    assert_eq!(pending_dream_jobs(&db, tid).await, 0);
}

// ── Task 26: POST /v1/dream/run enqueues a kind='dream' job and returns {job_id} ──────────────
#[sqlx::test(migrations = "../migrations")]
async fn test_dream_run_surface_returns_job_id(pool: PgPool) {
    use tower::ServiceExt;
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "dream-run").await;
    let tok = issue_access(&db, tid, uid).await;
    let res = app
        .oneshot(
            axum::http::Request::post("/v1/dream/run")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let body = body_string(res).await;
    assert!(body.contains("job_id"), "manual run returns a job id: {body}");
    // the kind='dream' job row landed (carrying user_id + current_session for the handler).
    assert_eq!(pending_dream_jobs(&db, tid).await, 1);
}

// ── Task 30/26: the Dream SSE stream mounts + accepts ?token= auth ────────────────────────────
#[sqlx::test(migrations = "../migrations")]
async fn test_dream_sse_stream_accepts_token(pool: PgPool) {
    use tower::ServiceExt;
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "dream-sse").await;
    let tok = issue_access(&db, tid, uid).await;
    let job = Uuid::now_v7();
    let res = app
        .oneshot(
            axum::http::Request::get(format!("/v1/dream/{job}/stream?token={tok}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK, "dream SSE mounts + ?token= authenticates");
}

// ── the cron is gated OFF in dev: dream_enabled=false → zero enqueues, zero $ burned (#16) ─────
#[sqlx::test(migrations = "../migrations")]
async fn test_dream_cron_gated_off_is_noop(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, _uid) = seed_tenant(&db, "cron-off").await;
    seed_clock_open(&db, tid).await; // would be due — but the gate is off

    // the default test config has dream_enabled=false.
    let st = app_state(&db);
    assert!(!st.cfg.dream_enabled);
    let n = tick_once(&st).await.unwrap();
    assert_eq!(n, 0, "gated off → the cron never enqueues (pitfall #16)");
    assert_eq!(pending_dream_jobs(&db, tid).await, 0);
}
