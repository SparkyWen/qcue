#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue D4 — RoutedTranscriber provider SELECTION, driven through /v1/transcribe with the REAL
// transcriber: explicit setting overrides auto-derive; deepseek (no STT) is skipped/errored; no
// STT-capable key returns an actionable envelope. Seeded creds use placeholder ciphertext, so the
// selected provider's key does not decrypt → we assert the SELECTED provider + the no-usable-key
// envelope (which also proves no network call was made), not a real transcript.
mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use sqlx::PgPool;
use tower::ServiceExt;

async fn transcribe_json(app: axum::Router, tok: &str) -> serde_json::Value {
    let audio_b64 = base64::engine::general_purpose::STANDARD.encode(b"RIFF\0\0\0\0WAVEfmt ");
    let res = app
        .oneshot(
            Request::post("/v1/transcribe")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"audio_b64":"{audio_b64}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    serde_json::from_str(&body_string(res).await).unwrap()
}

#[sqlx::test(migrations = "../migrations")]
async fn auto_derives_stt_capable_provider_skipping_deepseek(pool: PgPool) {
    let db = from_pool(pool);
    let app = routed_test_router(&db);
    let (tid, uid) = seed_tenant(&db, "stt-sel-a").await;
    insert_cred(&db, tid, "deepseek", "ds-1", "ok", None).await; // not STT-capable → skipped
    insert_cred(&db, tid, "groq", "gq-1", "ok", None).await; // STT-capable → selected
    let tok = issue_access(&db, tid, uid).await;
    let v = transcribe_json(app, &tok).await;
    assert_eq!(v["provider"], "groq", "must select the STT-capable provider, not deepseek");
    // placeholder ciphertext can't be unsealed → no-usable-key envelope (proves no network call)
    assert_eq!(v["success"], false);
    assert!(v["error"].as_str().unwrap().contains("no usable groq key"));
}

#[sqlx::test(migrations = "../migrations")]
async fn explicit_setting_overrides_auto_derive(pool: PgPool) {
    let db = from_pool(pool);
    let app = routed_test_router(&db);
    let (tid, uid) = seed_tenant(&db, "stt-sel-b").await;
    insert_cred(&db, tid, "openai", "oa-1", "ok", None).await; // would auto-derive to openai
    insert_cred(&db, tid, "zhipu", "zp-1", "ok", None).await;
    set_stt_provider(&db, tid, "zhipu").await; // explicit setting wins
    let tok = issue_access(&db, tid, uid).await;
    let v = transcribe_json(app, &tok).await;
    assert_eq!(v["provider"], "zhipu");
    assert!(v["error"].as_str().unwrap().contains("no usable zhipu key"));
}

#[sqlx::test(migrations = "../migrations")]
async fn no_stt_capable_key_returns_actionable_error(pool: PgPool) {
    let db = from_pool(pool);
    let app = routed_test_router(&db);
    let (tid, uid) = seed_tenant(&db, "stt-sel-c").await;
    insert_cred(&db, tid, "deepseek", "ds-1", "ok", None).await; // only a non-STT key
    let tok = issue_access(&db, tid, uid).await;
    let v = transcribe_json(app, &tok).await;
    assert_eq!(v["success"], false);
    assert!(v["error"].as_str().unwrap().to_lowercase().contains("no speech-to-text provider"));
}

#[sqlx::test(migrations = "../migrations")]
async fn explicit_non_stt_provider_is_a_clear_error(pool: PgPool) {
    let db = from_pool(pool);
    let app = routed_test_router(&db);
    let (tid, uid) = seed_tenant(&db, "stt-sel-d").await;
    insert_cred(&db, tid, "deepseek", "ds-1", "ok", None).await;
    set_stt_provider(&db, tid, "deepseek").await; // explicitly chose a non-STT provider
    let tok = issue_access(&db, tid, uid).await;
    let v = transcribe_json(app, &tok).await;
    assert_eq!(v["success"], false);
    assert!(v["error"].as_str().unwrap().contains("doesn't support voice"));
}
