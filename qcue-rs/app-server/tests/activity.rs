// QCue S3 — the Activity surface tests: approvals list + respond (D13, reversible), jobs list, cost
// today + ledger. Asserts the exact JSON shapes the Flutter `Approval`/`JobRow`/`CostLedgerRow` decoders
// expect, the reject-restores-soft-delete reversibility (pitfall #18), and RLS isolation.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn get(app: &axum::Router, path: &str, tok: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::get(path)
                .header("authorization", format!("Bearer {tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn post(app: &axum::Router, path: &str, tok: &str, body: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::post(path)
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

// ── approvals list returns pending rows in the Approval shape ──
#[sqlx::test(migrations = "../migrations")]
async fn test_approvals_list_shape(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "appr-list").await;
    insert_approval(&db, tid, uid, "wiki_merge", "dream",
        json!({"from": Uuid::now_v7().to_string(), "into": Uuid::now_v7().to_string()})).await;
    let tok = issue_access(&db, tid, uid).await;
    let res = get(&app, "/v1/approvals", &tok).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let arr = v["approvals"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    let a = &arr[0];
    assert!(a["id"].is_string());
    assert_eq!(a["action"], "wiki_merge");
    assert_eq!(a["status"], "pending");
    assert_eq!(a["requested_by"], "dream");
    assert!(a["subject_ref"].is_object(), "subject_ref is the raw JSONB map");
}

// ── approve finalizes the candidate (status flips, soft-delete stands) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_approval_approve(pool: PgPool) {
    let db = from_pool(pool);
    let root = test_data_root();
    let app = test_router_at(&db, &root).await;
    let (tid, uid) = seed_tenant(&db, "appr-ok").await;
    // a delete candidate whose page is already soft-deleted (the gate soft-deletes at propose time).
    let page = insert_wiki_page(&db, &root, tid, "concept", "stale", "Stale", "x", "## Stale\n", &[], &[]).await;
    soft_delete_page(&db, tid, page).await;
    let id = insert_approval(&db, tid, uid, "wiki_delete", "dream", json!({"page": page.to_string()})).await;
    let tok = issue_access(&db, tid, uid).await;
    let res = post(&app, &format!("/v1/approvals/{id}"), &tok, r#"{"approve":true}"#).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(v["status"], "approved");
    // the page stays soft-deleted (the delete is now canonical).
    assert!(page_deleted_at(&db, tid, page).await.is_some(), "approve keeps the soft-delete");
    // the pending list is now empty.
    let res = get(&app, "/v1/approvals", &tok).await;
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(v["approvals"].as_array().unwrap().len(), 0);
}

// ── reject reverses the destructive op: restores the soft-deleted page (D13 reversibility) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_approval_reject_restores(pool: PgPool) {
    let db = from_pool(pool);
    let root = test_data_root();
    let app = test_router_at(&db, &root).await;
    let (tid, uid) = seed_tenant(&db, "appr-no").await;
    let from = insert_wiki_page(&db, &root, tid, "concept", "dup", "Dup", "x", "## Dup\n", &[], &[]).await;
    let into = insert_wiki_page(&db, &root, tid, "concept", "canon", "Canon", "y", "## Canon\n", &[], &[]).await;
    soft_delete_page(&db, tid, from).await; // merge soft-deletes the `from` page
    let id = insert_approval(&db, tid, uid, "wiki_merge", "dream",
        json!({"from": from.to_string(), "into": into.to_string()})).await;
    let tok = issue_access(&db, tid, uid).await;
    let res = post(&app, &format!("/v1/approvals/{id}"), &tok, r#"{"approve":false}"#).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(v["status"], "rejected");
    // the `from` page is RESTORED (deleted_at back to NULL); the merge is fully undone.
    assert!(page_deleted_at(&db, tid, from).await.is_none(), "reject restores the soft-deleted page");
}

// ── respond rejects an unknown field (deny_unknown_fields) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_approval_respond_deny_unknown(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "appr-deny").await;
    let id = insert_approval(&db, tid, uid, "wiki_delete", "dream", json!({"page": Uuid::now_v7().to_string()})).await;
    let tok = issue_access(&db, tid, uid).await;
    let res = post(&app, &format!("/v1/approvals/{id}"), &tok, r#"{"approve":true,"force":true}"#).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "deny_unknown_fields rejects extra keys");
}

// ── jobs list maps to the JobRow shape (id/kind/state + nullable progress/last_error) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_jobs_list_shape(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "jobs-list").await;
    insert_job(&db, tid, "dream", "leased", Some(json!({"progress": 0.6}))).await;
    insert_job(&db, tid, "ingest", "done", None).await;
    let tok = issue_access(&db, tid, uid).await;
    let res = get(&app, "/v1/jobs", &tok).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let jobs = v["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    let leased = jobs.iter().find(|j| j["kind"] == "dream").unwrap();
    assert_eq!(leased["state"], "leased");
    assert_eq!(leased["progress"], 0.6, "progress is read from result->>'progress'");
    let done = jobs.iter().find(|j| j["kind"] == "ingest").unwrap();
    assert!(done.get("progress").is_none(), "a job with no progress omits the field (nullable in Dart)");
}

// ── cost today is the tenant-scope micros sum (fresh day = 0) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_cost_today(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "cost-today").await;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    insert_cost_row(&db, tid, &today, 12400, 3210, 8100, 1200, 640, 420000).await;
    let tok = issue_access(&db, tid, uid).await;
    let res = get(&app, "/v1/cost/today", &tok).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(v["cost_micros"], 420000);
}

// ── cost ledger returns recent rows with all 5 token kinds + cost_micros (CostLedgerRow shape) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_cost_ledger_shape(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "cost-ledger").await;
    insert_cost_row(&db, tid, "2026-06-13", 12400, 3210, 8100, 1200, 640, 420000).await;
    insert_cost_row(&db, tid, "2026-06-12", 31002, 8114, 15400, 2200, 1980, 1070000).await;
    let tok = issue_access(&db, tid, uid).await;
    let res = get(&app, "/v1/cost/ledger", &tok).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let r = &rows[0]; // newest first → 06-13
    assert!(r["day"].as_str().unwrap().starts_with("2026-06-13"), "day is an ISO8601 timestamp string");
    assert_eq!(r["input_tokens"], 12400);
    assert_eq!(r["output_tokens"], 3210);
    assert_eq!(r["cache_read_tokens"], 8100);
    assert_eq!(r["cache_write_tokens"], 1200);
    assert_eq!(r["reasoning_tokens"], 640, "the 5th CanonicalUsage field is present");
    assert_eq!(r["cost_micros"], 420000);
}

// ── RLS: tenant A sees neither B's approvals/jobs nor B's cost ──
#[sqlx::test(migrations = "../migrations")]
async fn test_activity_tenant_isolation(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (a, ua) = seed_tenant(&db, "act-iso-a").await;
    let (b, ub) = seed_tenant(&db, "act-iso-b").await;
    insert_approval(&db, b, ub, "wiki_delete", "dream", json!({"page": Uuid::now_v7().to_string()})).await;
    insert_job(&db, b, "ingest", "done", None).await;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    insert_cost_row(&db, b, &today, 1, 1, 1, 1, 1, 99999).await;
    let tok = issue_access(&db, a, ua).await; // A's token
    let v: serde_json::Value = serde_json::from_str(&body_string(get(&app, "/v1/approvals", &tok).await).await).unwrap();
    assert_eq!(v["approvals"].as_array().unwrap().len(), 0, "RLS hides B's approvals");
    let v: serde_json::Value = serde_json::from_str(&body_string(get(&app, "/v1/jobs", &tok).await).await).unwrap();
    assert_eq!(v["jobs"].as_array().unwrap().len(), 0, "RLS hides B's jobs");
    let v: serde_json::Value = serde_json::from_str(&body_string(get(&app, "/v1/cost/today", &tok).await).await).unwrap();
    assert_eq!(v["cost_micros"], 0, "RLS hides B's cost from A");
}
