#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use app_server::config::{Config, ConfigError};

#[test]
fn test_refuse_boot_weak_secret() {
    let mut raw = Config::test_raw();
    raw.jwt_secret = "short16byteslong".to_string(); // 16 bytes
    assert!(matches!(Config::validate(raw), Err(ConfigError::WeakSecret)));
}
#[test]
fn test_db_url_isolation() {
    let mut raw = Config::test_raw();
    raw.database_url = String::new();
    assert!(matches!(Config::validate(raw), Err(ConfigError::MissingDatabaseUrl)));
}
#[test]
fn test_bind_config() {
    let raw = Config::test_raw();
    let cfg = Config::validate(raw).unwrap();
    assert_eq!(cfg.bind_addr, "127.0.0.1"); // loopback default
}

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

#[sqlx::test(migrations = "../migrations")]
async fn test_security_headers_and_origin_reject(pool: PgPool) {
    let db = common::from_pool(pool);
    let app = common::test_router(&db).await;
    let res = app.clone().oneshot(Request::get("/healthz").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.headers().get("x-content-type-options").and_then(|v| v.to_str().ok()), Some("nosniff"));
    assert_eq!(res.headers().get("x-frame-options").and_then(|v| v.to_str().ok()), Some("DENY"));
    // foreign Origin on the WSS/SSE upgrade path → 403 (S3-R65)
    let bad = app
        .oneshot(
            Request::get("/v1/recall/00000000-0000-7000-8000-000000000000/stream")
                .header(header::ORIGIN, "https://evil.example")
                .header(header::AUTHORIZATION, "Bearer x")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "../migrations")]
async fn test_body_limit_and_health(pool: PgPool) {
    let db = common::from_pool(pool);
    let app = common::test_router(&db).await;
    let big = "x".repeat(300 * 1024);
    let res = app
        .clone()
        .oneshot(
            Request::post("/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"email":"a@b.c","password":"{big}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let h = app.oneshot(Request::get("/healthz").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(h.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "../migrations")]
async fn test_dto_deny_unknown(pool: PgPool) {
    let db = common::from_pool(pool);
    let app = common::test_router(&db).await;
    let (tid, uid) = common::seed_tenant(&db, "dto-a").await;
    let tok = common::issue_access(&db, tid, uid).await;
    // an extra field on the capture DTO → 422 (B-R8)
    let res = app
        .oneshot(
            Request::post("/v1/capture")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind":"text","body":"x","origin":"capture","evil":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(res.status() == StatusCode::UNPROCESSABLE_ENTITY || res.status() == StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../migrations")]
async fn test_readyz_ok_when_migrated(pool: PgPool) {
    let db = common::from_pool(pool);
    let app = common::test_router(&db).await;
    let r = app.oneshot(Request::get("/readyz").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "readyz 200 once DB + migrations are up");
}

#[sqlx::test(migrations = "../migrations")]
async fn test_rate_limit(pool: PgPool) {
    app_server::middleware::reset_rate_limit();
    let db = common::from_pool(pool);
    let app = common::test_router(&db).await;
    // /v1/auth/* throttles at 20/60s. The 21st request from the same peer → 429 with Retry-After.
    let mut last = StatusCode::OK;
    for _ in 0..25 {
        let res = app
            .clone()
            .oneshot(
                Request::post("/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"email":"nobody@example.com","password":"x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        last = res.status();
        if last == StatusCode::TOO_MANY_REQUESTS {
            assert!(res.headers().get(header::RETRY_AFTER).is_some(), "429 carries Retry-After");
            break;
        }
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS, "auth bucket must throttle after 20/60s");
    app_server::middleware::reset_rate_limit();
}

#[test]
fn test_trust_proxy_ip_hash_redacts() {
    // ip_hash never reveals the raw IP (privacy, S3-R64): different IPs → different stable hashes.
    let a = app_server::middleware::ip_hash("203.0.113.7");
    let b = app_server::middleware::ip_hash("203.0.113.8");
    assert_ne!(a, b);
    assert_eq!(a, app_server::middleware::ip_hash("203.0.113.7"), "hash is stable");
    assert!(!a.contains("203.0.113"), "raw IP must not appear in the hash");
}
