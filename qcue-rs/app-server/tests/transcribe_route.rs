#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue D4 — POST /v1/transcribe: auth'd, decodes base64 audio, returns the transcript from the
// per-tenant Transcriber seam (StubTranscriber in tests). Bad base64 → 400.
mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use sqlx::PgPool;
use tower::ServiceExt;

#[sqlx::test(migrations = "../migrations")]
async fn test_transcribe_returns_transcript(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "stt-a").await;
    let tok = issue_access(&db, tid, uid).await;
    let audio_b64 = base64::engine::general_purpose::STANDARD.encode(b"\x00\x01\x02 fake m4a bytes");
    let res = app
        .oneshot(
            Request::post("/v1/transcribe")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"audio_b64":"{audio_b64}","language":"zh"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["success"], true);
    assert_eq!(v["transcript"], STUB_TRANSCRIPT);
}

#[sqlx::test(migrations = "../migrations")]
async fn test_transcribe_rejects_bad_base64(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "stt-b").await;
    let tok = issue_access(&db, tid, uid).await;
    let res = app
        .oneshot(
            Request::post("/v1/transcribe")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"audio_b64":"!!!not base64!!!"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../migrations")]
async fn test_transcribe_requires_auth(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let audio_b64 = base64::engine::general_purpose::STANDARD.encode(b"audio");
    let res = app
        .oneshot(
            Request::post("/v1/transcribe")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"audio_b64":"{audio_b64}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
