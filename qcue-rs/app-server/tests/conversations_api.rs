//! REC-R3/REC-R4 — the two recall-history read endpoints, tenant-scoped (RLS) + redacted.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt; // oneshot
use uuid::Uuid;

#[sqlx::test(migrations = "../migrations")]
async fn list_and_get_conversations_are_tenant_scoped_and_redacted(pool: PgPool) {
    let db = from_pool(pool);
    let (a, ua) = seed_tenant(&db, "convo-api-a").await;
    let (b, _ub) = seed_tenant(&db, "convo-api-b").await;
    let st = app_state(&db);
    let thread = Uuid::now_v7();
    // a real persisted recall turn for tenant A.
    app_server::recall::run_recall_stream(&st, a, ua, thread, "What did I decide about indexing?", app_server::recall::RecallMode::Recall, Default::default()).await;

    let router = app_server::router::build_router(st);
    let token_a = issue_access(&db, a, ua).await;
    let token_b = issue_access(&db, b, _ub).await;

    // GET /v1/conversations (tenant A) → one conversation, titled from the question.
    let res = router.clone().oneshot(
        Request::get("/v1/conversations").header("Authorization", format!("Bearer {token_a}")).body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(body.contains("What did I decide about indexing"), "title present: {body}");
    assert!(body.contains(&thread.to_string()), "thread id present");

    // Tenant B sees NO conversations (RLS isolation).
    let res_b = router.clone().oneshot(
        Request::get("/v1/conversations").header("Authorization", format!("Bearer {token_b}")).body(Body::empty()).unwrap()
    ).await.unwrap();
    let body_b = body_string(res_b).await;
    assert!(!body_b.contains(&thread.to_string()), "tenant B must not see tenant A's thread: {body_b}");

    // GET /v1/conversations/{thread}/messages → user+assistant turns, no tool_calls/provider_data keys.
    let res_m = router.clone().oneshot(
        Request::get(format!("/v1/conversations/{thread}/messages")).header("Authorization", format!("Bearer {token_a}")).body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(res_m.status(), StatusCode::OK);
    let body_m = body_string(res_m).await;
    assert!(body_m.contains("\"role\":\"user\""), "user turn present: {body_m}");
    assert!(body_m.contains("\"role\":\"assistant\""), "assistant turn present");
    assert!(!body_m.contains("tool_calls"), "tool_calls must be redacted/absent (REC-R4)");
    assert!(!body_m.contains("provider_data"), "provider_data must be redacted/absent (REC-R4)");
}
