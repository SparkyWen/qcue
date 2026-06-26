// QCue S3 — the Settings surface tests beyond the vault: model picker (`GET .../models/{provider}`,
// `GET/PUT .../models/{provider}/active`) + the server-Dream toggle (`GET/PUT /v1/settings/dream`).
// Asserts the `{models:[...]}` / `{model:...}` / `{enabled:...}` shapes the Flutter client decodes, the
// 404→null active-model path, deny_unknown_fields bodies, persistence, and RLS isolation.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

async fn get(app: &axum::Router, path: &str, tok: &str) -> axum::response::Response {
    app.clone()
        .oneshot(Request::get(path).header("authorization", format!("Bearer {tok}")).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn put(app: &axum::Router, path: &str, tok: &str, body: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::put(path)
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

// ── models list returns the static per-provider catalog ──
#[sqlx::test(migrations = "../migrations")]
async fn test_models_list(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "models-list").await;
    let tok = issue_access(&db, tid, uid).await;
    let res = get(&app, "/v1/settings/models/anthropic", &tok).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let models: Vec<&str> = v["models"].as_array().unwrap().iter().map(|m| m.as_str().unwrap()).collect();
    assert!(models.contains(&"claude-opus-4-8"), "the anthropic catalog is offered");
    // an unknown provider yields an empty list (the client renders "no models").
    let res = get(&app, "/v1/settings/models/unknownco", &tok).await;
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(v["models"].as_array().unwrap().len(), 0);
}

// ── active model: 404 when unset (→ null), then set→get round-trips ──
#[sqlx::test(migrations = "../migrations")]
async fn test_active_model_set_get(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "active-model").await;
    let tok = issue_access(&db, tid, uid).await;
    // unset → 404 (the Dart client maps it to null).
    let res = get(&app, "/v1/settings/models/openai/active", &tok).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    // set a valid model (gpt-5.5 is the curated openai flagship/default).
    let res = put(&app, "/v1/settings/models/openai/active", &tok, r#"{"model":"gpt-5.5"}"#).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(v["model"], "gpt-5.5");
    // get returns it.
    let res = get(&app, "/v1/settings/models/openai/active", &tok).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(v["model"], "gpt-5.5");
    // re-PUT a different valid model overwrites (versioned upsert). gpt-5.4-mini is the low-price catalog id.
    let res = put(&app, "/v1/settings/models/openai/active", &tok, r#"{"model":"gpt-5.4-mini"}"#).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(get(&app, "/v1/settings/models/openai/active", &tok).await).await).unwrap();
    assert_eq!(v["model"], "gpt-5.4-mini");
}

// ── set-active rejects an unknown model + an unknown body field ──
#[sqlx::test(migrations = "../migrations")]
async fn test_active_model_validation(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "active-val").await;
    let tok = issue_access(&db, tid, uid).await;
    // unknown model for the provider → 400.
    let res = put(&app, "/v1/settings/models/openai/active", &tok, r#"{"model":"not-a-real-model"}"#).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    // deny_unknown_fields → 422.
    let res = put(&app, "/v1/settings/models/openai/active", &tok, r#"{"model":"gpt-5.5","pin":true}"#).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ── server-Dream toggle defaults OFF, then set→get round-trips (D9) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_server_dream_toggle(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "dream-toggle").await;
    let tok = issue_access(&db, tid, uid).await;
    // default OFF (fail-closed server-readable posture).
    let v: serde_json::Value = serde_json::from_str(&body_string(get(&app, "/v1/settings/dream", &tok).await).await).unwrap();
    assert_eq!(v["enabled"], false);
    // turn it on.
    let res = put(&app, "/v1/settings/dream", &tok, r#"{"enabled":true}"#).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(v["enabled"], true);
    // persisted.
    let v: serde_json::Value = serde_json::from_str(&body_string(get(&app, "/v1/settings/dream", &tok).await).await).unwrap();
    assert_eq!(v["enabled"], true);
    // deny_unknown_fields on the toggle body.
    let res = put(&app, "/v1/settings/dream", &tok, r#"{"enabled":false,"scope":"all"}"#).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ── RLS: tenant A's active-model + dream toggle are invisible to tenant B ──
#[sqlx::test(migrations = "../migrations")]
async fn test_settings_tenant_isolation(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (a, ua) = seed_tenant(&db, "set-iso-a").await;
    let (b, ub) = seed_tenant(&db, "set-iso-b").await;
    let tok_a = issue_access(&db, a, ua).await;
    let tok_b = issue_access(&db, b, ub).await;
    // A sets its active model + dream on.
    put(&app, "/v1/settings/models/openai/active", &tok_a, r#"{"model":"gpt-5.5"}"#).await;
    put(&app, "/v1/settings/dream", &tok_a, r#"{"enabled":true}"#).await;
    // B sees neither (RLS scopes session_kv).
    let res = get(&app, "/v1/settings/models/openai/active", &tok_b).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "RLS hides A's active model from B");
    let v: serde_json::Value = serde_json::from_str(&body_string(get(&app, "/v1/settings/dream", &tok_b).await).await).unwrap();
    assert_eq!(v["enabled"], false, "RLS hides A's dream toggle from B (B reads its own default OFF)");
}
