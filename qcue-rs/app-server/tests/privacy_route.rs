// QCue — GET /privacy (+ /privacy/zh): the App Store privacy URL must resolve from the backend.
// Both routes are UNauthenticated (merged alongside /healthz, not behind the TenantCtx extractor)
// and serve the committed docs/legal/privacy-policy.{en,zh}.html, baked in via include_str!.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

#[sqlx::test(migrations = "../migrations")]
async fn privacy_en_served_unauthenticated(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;

    // No Authorization header — a public page must still 200.
    let res = app
        .clone()
        .oneshot(Request::get("/privacy").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "GET /privacy → 200 without auth");
    let ct = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(ct.starts_with("text/html"), "content-type is text/html, got {ct:?}");
    let body = body_string(res).await;
    assert!(body.contains("Privacy Policy"), "en privacy policy body served");
}

#[sqlx::test(migrations = "../migrations")]
async fn privacy_zh_served_unauthenticated(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;

    let res = app
        .clone()
        .oneshot(Request::get("/privacy/zh").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "GET /privacy/zh → 200 without auth");
    let body = body_string(res).await;
    // zh.html has NO ASCII "Privacy Policy" — assert the Chinese title instead.
    assert!(body.contains("隐私政策"), "zh privacy policy body served");
}
