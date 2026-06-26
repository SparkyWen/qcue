//! QCue v0.1.1 — server-side capture idempotency (migration 50002 + the `ON CONFLICT … WHERE` fix).
//!
//! Regression guard for the bug that 500'd EVERY capture: the handler's `ON CONFLICT (tenant_id,
//! idempotency_key)` omitted the partial index's `WHERE idempotency_key IS NOT NULL` predicate, so
//! Postgres couldn't infer the arbiter. These tests prove (a) a keyless capture still works and (b) two
//! POSTs carrying the SAME `Idempotency-Key` dedup to ONE idea row with the SAME `idea_id` — the
//! offline-queue retry safety the app relies on.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use tower::ServiceExt;
use uuid::Uuid;

async fn post_capture(app: &axum::Router, tok: &str, body: &str, idem: Option<&str>) -> (axum::http::StatusCode, String) {
    let mut req = axum::http::Request::post("/v1/capture")
        .header("authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json");
    if let Some(k) = idem {
        req = req.header("Idempotency-Key", k);
    }
    let res = app
        .clone()
        .oneshot(req.body(axum::body::Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = res.status();
    (status, body_string(res).await)
}

#[sqlx::test(migrations = "../migrations")]
async fn same_idempotency_key_dedups_to_one_row(pool: sqlx::PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "idem-a").await;
    let tok = issue_access(&db, tid, uid).await;
    let body = r#"{"kind":"text","body":"my note","origin":"capture"}"#;

    // First POST with a key → 200, creates one row.
    let (s1, b1) = post_capture(&app, &tok, body, Some("key-123")).await;
    assert_eq!(s1, axum::http::StatusCode::OK, "first keyed capture: {b1}");
    let id1 = idea_id_of(&b1);

    // Second POST with the SAME key (a retried flush) → 200, same idea_id, NO new row/job.
    let (s2, b2) = post_capture(&app, &tok, body, Some("key-123")).await;
    assert_eq!(s2, axum::http::StatusCode::OK, "retried keyed capture: {b2}");
    let id2 = idea_id_of(&b2);
    assert_eq!(id1, id2, "the same Idempotency-Key returns the same idea_id");

    // Exactly one idea row + one ingest job for this tenant.
    let mut tx = tenant_tx(&db, tid).await;
    let ideas: i64 = sqlx::query_scalar("SELECT count(*) FROM ideas WHERE tenant_id=$1").bind(tid).fetch_one(&mut *tx).await.unwrap();
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE tenant_id=$1 AND kind='ingest'").bind(tid).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(ideas, 1, "idempotent retry inserts exactly one idea row");
    assert_eq!(jobs, 1, "idempotent retry enqueues exactly one ingest job");
}

#[sqlx::test(migrations = "../migrations")]
async fn keyless_captures_are_each_their_own_row(pool: sqlx::PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "idem-b").await;
    let tok = issue_access(&db, tid, uid).await;
    let body = r#"{"kind":"text","body":"n","origin":"capture"}"#;

    // Two keyless POSTs → two distinct rows (NULL keys are excluded from the unique index).
    let (s1, b1) = post_capture(&app, &tok, body, None).await;
    let (s2, b2) = post_capture(&app, &tok, body, None).await;
    assert_eq!(s1, axum::http::StatusCode::OK, "{b1}");
    assert_eq!(s2, axum::http::StatusCode::OK, "{b2}");
    assert_ne!(idea_id_of(&b1), idea_id_of(&b2), "keyless captures are distinct rows");
}

fn idea_id_of(body: &str) -> Uuid {
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    Uuid::parse_str(v["idea_id"].as_str().unwrap()).unwrap()
}
