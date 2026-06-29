#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue D4 — the STT settings + capability surface: PUT/GET /v1/settings/stt-provider (roundtrip +
// reject non-STT) and GET /v1/transcribe/providers (lists only configured STT-capable providers).
mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

#[sqlx::test(migrations = "../migrations")]
async fn put_then_get_roundtrips_stt_provider(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "stt-set").await;
    let tok = issue_access(&db, tid, uid).await;
    let put = app
        .clone()
        .oneshot(
            Request::put("/v1/settings/stt-provider")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"provider":"zhipu"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::OK);
    let get = app
        .oneshot(
            Request::get("/v1/settings/stt-provider")
                .header("authorization", format!("Bearer {tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&body_string(get).await).unwrap();
    assert_eq!(v["provider"], "zhipu");
}

#[sqlx::test(migrations = "../migrations")]
async fn put_rejects_non_stt_provider(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "stt-set-bad").await;
    let tok = issue_access(&db, tid, uid).await;
    let put = app
        .oneshot(
            Request::put("/v1/settings/stt-provider")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"provider":"deepseek"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../migrations")]
async fn get_unset_returns_null(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "stt-set-null").await;
    let tok = issue_access(&db, tid, uid).await;
    let get = app
        .oneshot(
            Request::get("/v1/settings/stt-provider")
                .header("authorization", format!("Bearer {tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&body_string(get).await).unwrap();
    assert!(v["provider"].is_null());
}

#[sqlx::test(migrations = "../migrations")]
async fn providers_endpoint_lists_configured_stt_capable_only(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "stt-list").await;
    insert_cred(&db, tid, "deepseek", "ds-1", "ok", None).await; // not STT-capable
    insert_cred(&db, tid, "openai", "oa-1", "ok", None).await; // STT-capable
    let tok = issue_access(&db, tid, uid).await;
    let res = app
        .oneshot(
            Request::get("/v1/transcribe/providers")
                .header("authorization", format!("Bearer {tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let available: Vec<String> = serde_json::from_value(v["available"].clone()).unwrap();
    assert!(available.contains(&"openai".to_string()), "available={available:?}");
    assert!(!available.contains(&"deepseek".to_string()), "deepseek is not STT-capable");
    let all_capable: Vec<String> = serde_json::from_value(v["all_capable"].clone()).unwrap();
    assert!(all_capable.contains(&"qwen".to_string()));
    assert!(all_capable.contains(&"gemini".to_string()));
    assert!(!all_capable.contains(&"minimax".to_string()), "MiniMax removed from the STT list");
}

#[sqlx::test(migrations = "../migrations")]
async fn put_auto_is_accepted_and_returned(pool: PgPool) {
    // "auto" is a valid choice (means auto-derive) and must NOT be rejected like a non-STT provider.
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "stt-set-auto").await;
    let tok = issue_access(&db, tid, uid).await;
    let put = app
        .clone()
        .oneshot(
            Request::put("/v1/settings/stt-provider")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"provider":"auto"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::OK);
    let get = app
        .oneshot(
            Request::get("/v1/settings/stt-provider")
                .header("authorization", format!("Bearer {tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&body_string(get).await).unwrap();
    assert_eq!(v["provider"], "auto");
}
