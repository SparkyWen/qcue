//! QCue DIG-R2/DIG-R3 — POST /v1/ingest/run enqueues a debounced per-idea ingest job for each dirty
//! capture (pending OR edited-since-ingest), returns {enqueued, job_ids}, and a double-click collapses
//! to one job per idea (debounce_ref="ingest:{id}").
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use tower::ServiceExt;
use uuid::Uuid;

async fn post_run(app: &axum::Router, tok: &str) -> (axum::http::StatusCode, String) {
    let req = axum::http::Request::post("/v1/ingest/run")
        .header("authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json");
    let res = app.clone().oneshot(req.body(axum::body::Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    (status, body_string(res).await)
}

/// Seed a `pending` idea directly (the capture surface would have done the insert + auto-enqueue; here
/// we want a clean pending row with NO existing ingest job so the run is the first enqueue).
async fn seed_pending_idea(db: &TestDb, tid: Uuid, uid: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    let mut tx = tenant_tx(db, tid).await;
    sqlx::query(
        "INSERT INTO ideas(id,tenant_id,user_id,kind,body,log_ref,origin,ingest_state) \
         VALUES ($1,$2,$3,'text','a note',$4,'capture','pending')",
    )
    .bind(id).bind(tid).bind(uid).bind(format!("captures/{id}.jsonl"))
    .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    id
}

#[sqlx::test(migrations = "../migrations")]
async fn run_enqueues_per_dirty_idea(pool: sqlx::PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "dig-a").await;
    let tok = issue_access(&db, tid, uid).await;
    let i1 = seed_pending_idea(&db, tid, uid).await;
    let i2 = seed_pending_idea(&db, tid, uid).await;

    let (s, b) = post_run(&app, &tok).await;
    assert_eq!(s, axum::http::StatusCode::OK, "{b}");
    let v: serde_json::Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["enqueued"].as_u64().unwrap(), 2, "two dirty ideas enqueued: {b}");
    assert_eq!(v["job_ids"].as_array().unwrap().len(), 2);

    // exactly two ingest jobs exist (one per dirty idea), each carrying its debounce_ref.
    let mut tx = tenant_tx(&db, tid).await;
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE tenant_id=$1 AND kind='ingest'")
        .bind(tid).fetch_one(&mut *tx).await.unwrap();
    let refs: Vec<(String,)> = sqlx::query_as(
        "SELECT payload->>'debounce_ref' FROM jobs WHERE tenant_id=$1 AND kind='ingest' ORDER BY 1",
    ).bind(tid).fetch_all(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(jobs, 2);
    let mut expected = vec![format!("ingest:{i1}"), format!("ingest:{i2}")];
    expected.sort();
    let got: Vec<String> = refs.into_iter().map(|r| r.0).collect();
    assert_eq!(got, expected, "each job carries debounce_ref ingest:{{id}}");
}

#[sqlx::test(migrations = "../migrations")]
async fn double_click_collapses_to_one_job_per_idea(pool: sqlx::PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "dig-b").await;
    let tok = issue_access(&db, tid, uid).await;
    let _ = seed_pending_idea(&db, tid, uid).await;

    let (s1, b1) = post_run(&app, &tok).await;
    let (s2, b2) = post_run(&app, &tok).await;
    assert_eq!(s1, axum::http::StatusCode::OK, "{b1}");
    assert_eq!(s2, axum::http::StatusCode::OK, "{b2}");

    // the pending ingest job from the first click is still pending → the second click DEBOUNCES onto it.
    let mut tx = tenant_tx(&db, tid).await;
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE tenant_id=$1 AND kind='ingest'")
        .bind(tid).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(jobs, 1, "repeated clicks collapse to one pending job per idea (DIG-R3)");
}
