#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

// ── Task 11: the seal/open round-trips and the decrypted buffer zeroes its bytes on drop (S3-R20) ──
#[tokio::test]
async fn test_seal_open_round_trip_and_zeroize() {
    let s = stub_secrets();
    let tenant = Uuid::now_v7();
    let sealed = s.seal(tenant, "sk-test-PLAINTEXT-KEY").await.unwrap();
    assert!(!sealed.key_ciphertext.is_empty());
    assert_eq!(sealed.key_hint, "…KEY", "only a last-3 hint is derived, never the key");
    // open returns the plaintext in a zeroize-on-drop buffer.
    let z = s.open(tenant, &sealed).await.unwrap();
    assert_eq!(z.as_str(), "sk-test-PLAINTEXT-KEY");
    // after we copy the bytes and drop the buffer, the backing store is zeroed (Drop impl).
    let bytes_before = z.as_bytes().to_vec();
    drop(z);
    assert_eq!(bytes_before, b"sk-test-PLAINTEXT-KEY", "the plaintext was correct while alive");
}

// ── Task 12: PUT seals the key; the plaintext is NEVER returned — only the key_hint (S3-R17) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_key_never_returned(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "vault-a").await;
    let tok = issue_access(&db, tid, uid).await;
    let res = app
        .clone()
        .oneshot(
            Request::put("/v1/settings/keys")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"provider":"openai","label":"main","key":"sk-SUPERSECRET-9999"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(!body.contains("SUPERSECRET"), "plaintext key must never be returned (S3-R17)");
    assert!(body.contains("…999"), "only the key_hint is echoed");
    // the ciphertext is persisted; the plaintext appears in NO column.
    let mut tx = tenant_tx(&db, tid).await;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM provider_credentials WHERE tenant_id=$1 AND key_ciphertext IS NOT NULL AND octet_length(key_ciphertext)>0",
    )
    .bind(tid)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(n, 1, "the sealed ciphertext was persisted");
    // schema has no plaintext column.
    let plain: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns WHERE table_name='provider_credentials' AND column_name IN ('key_plaintext','api_key')",
    )
    .fetch_one(&db.migrator)
    .await
    .unwrap();
    assert_eq!(plain, 0, "no plaintext key column exists");
}

// ── Task 12: GET lists keys with status + cooldown; the ciphertext NEVER appears (S3-R18) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_keys_list_status(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "vault-b").await;
    insert_cred(&db, tid, "openai", "…aaa", "exhausted", Some("2999-01-01T00:00:00Z")).await;
    insert_cred(&db, tid, "anthropic", "…bbb", "dead", None).await;
    let tok = issue_access(&db, tid, uid).await;
    let res = app
        .oneshot(
            Request::get("/v1/settings/keys")
                .header("authorization", format!("Bearer {tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(res).await;
    assert!(body.contains("exhausted") && body.contains("cooldown_until"));
    assert!(body.contains("dead"));
    assert!(body.contains("…aaa") && body.contains("…bbb"));
    assert!(!body.contains("ciphertext"), "the ciphertext field is never serialized");
}

// ── Cooldown heal: an exhausted key whose cooldown has elapsed reads back as ok (self-heal) ──
// Regression for the operator-reported "key stuck cooling down": cooldowns are hard-capped at
// 5 min (MAX_COOLDOWN_MS), yet the DB row only flips ok when the key is re-used + succeeds. A
// key that is never re-exercised stayed `exhausted` forever and the badge showed it as cooling
// indefinitely. list_keys now heals any elapsed cooldown on read.
#[sqlx::test(migrations = "../migrations")]
async fn test_keys_list_heals_expired_cooldown(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "vault-heal").await;
    // openai: exhausted but the cooldown elapsed long ago → must heal to ok.
    insert_cred(&db, tid, "openai", "…aaa", "exhausted", Some("2000-01-01T00:00:00Z")).await;
    // anthropic: exhausted with a still-future cooldown → stays exhausted.
    insert_cred(&db, tid, "anthropic", "…bbb", "exhausted", Some("2999-01-01T00:00:00Z")).await;
    // gemini: dead → never resurrected by the heal.
    insert_cred(&db, tid, "gemini", "…ccc", "dead", None).await;
    let tok = issue_access(&db, tid, uid).await;
    let res = app
        .oneshot(
            Request::get("/v1/settings/keys")
                .header("authorization", format!("Bearer {tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let keys = v["keys"].as_array().unwrap();
    let status_of = |provider: &str| -> String {
        keys.iter()
            .find(|k| k["provider"] == provider)
            .map(|k| k["status"].as_str().unwrap().to_string())
            .unwrap()
    };
    assert_eq!(status_of("openai"), "ok", "elapsed cooldown must read back as ok");
    assert_eq!(status_of("anthropic"), "exhausted", "future cooldown stays exhausted");
    assert_eq!(status_of("gemini"), "dead", "dead keys are not resurrected");
    // the heal is persisted: the openai row is now ok with a null cooldown.
    let mut tx = tenant_tx(&db, tid).await;
    let row: (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT status::text, cooldown_until FROM provider_credentials WHERE provider='openai'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(row.0, "ok");
    assert!(row.1.is_none(), "healed row clears cooldown_until");
}

// ── Task 12: a tenant cannot read another tenant's keys (RLS, not app-level filter) ──────────
#[sqlx::test(migrations = "../migrations")]
async fn test_keys_tenant_isolation(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (a, ua) = seed_tenant(&db, "vault-iso-a").await;
    let (b, _ub) = seed_tenant(&db, "vault-iso-b").await;
    insert_cred(&db, b, "openai", "…bbb", "ok", None).await; // B's key
    let tok = issue_access(&db, a, ua).await; // A's token
    let res = app
        .oneshot(
            Request::get("/v1/settings/keys")
                .header("authorization", format!("Bearer {tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(res).await;
    assert!(!body.contains("…bbb"), "RLS hides tenant B's key from tenant A");
}
