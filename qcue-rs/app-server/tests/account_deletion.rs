// QCue — DELETE /v1/account (Apple Guideline 5.1.1(v): in-app account deletion).
// Proves the authenticated endpoint (1) purges the caller's tenant row, (2) cascades to all
// per-tenant child data (provider_credentials, sessions are the representative children), and
// (3) rejects an unauthenticated caller with 401. Mirrors tests/auth.rs (Bearer header → 200,
// no-token → 401) + the common fixture helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt; // .oneshot()
use uuid::Uuid;

/// GUC-bound row count for a FORCE-RLS child table — a bare count WITHOUT the tenant GUC sees
/// zero rows (RLS filters everything) and would FALSE-POSITIVE a "cascade worked" assertion.
async fn child_count(db: &TestDb, tenant: Uuid, table: &str, col: &str) -> i64 {
    let mut tx = tenant_tx(db, tenant).await;
    let sql = format!("SELECT count(*) FROM {table} WHERE {col}=$1");
    let n: i64 = sqlx::query_scalar(&sql).bind(tenant).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    n
}

#[sqlx::test(migrations = "../migrations")]
async fn delete_account_purges_tenant_and_cascades(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "del-acct").await;
    // Seed representative child data across the cascade. issue_access also inserts a live
    // sessions row, so sessions is a second cascade witness.
    insert_cred(&db, tid, "openai", "sk-***1234", "ok", None).await;
    let access = issue_access(&db, tid, uid).await;

    // Pre-condition (GUC-bound so RLS doesn't hide the rows): the children exist.
    assert_eq!(child_count(&db, tid, "provider_credentials", "tenant_id").await, 1, "cred seeded");
    assert!(child_count(&db, tid, "sessions", "tenant_id").await >= 1, "session seeded");
    assert_eq!(child_count(&db, tid, "users", "tenant_id").await, 1, "user seeded");

    // DELETE /v1/account with the Bearer access token → 200 {"ok": true}.
    let res = app
        .clone()
        .oneshot(
            Request::delete("/v1/account")
                .header("authorization", format!("Bearer {access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "authenticated account delete → 200");
    let body = body_string(res).await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["ok"], serde_json::json!(true), "body is {{\"ok\":true}}");

    // The tenant root row is gone (tenants is RLS-free → read on the migrator pool).
    let still: Option<Uuid> = sqlx::query_scalar("SELECT id FROM tenants WHERE id=$1")
        .bind(tid)
        .fetch_optional(&db.migrator)
        .await
        .unwrap();
    assert!(still.is_none(), "tenant row purged");

    // The cascade emptied every per-tenant child (GUC-bound counts, now 0).
    assert_eq!(child_count(&db, tid, "provider_credentials", "tenant_id").await, 0, "creds cascade-deleted");
    assert_eq!(child_count(&db, tid, "sessions", "tenant_id").await, 0, "sessions cascade-deleted");
    assert_eq!(child_count(&db, tid, "users", "tenant_id").await, 0, "users cascade-deleted");
}

#[sqlx::test(migrations = "../migrations")]
async fn delete_account_purges_on_disk_vault(pool: PgPool) {
    // The handler must also purge the per-tenant object-store dir on disk — it holds the wiki vault
    // (the .md SOURCE OF TRUTH) + the raw capture .jsonl logs. Deleting only the Postgres mirror
    // would leave the user's notes on the VPS disk, breaking the "delete all data" promise.
    let db = from_pool(pool);
    let data_root = test_data_root();
    let app = test_router_at(&db, &data_root).await;
    let (tid, uid) = seed_tenant(&db, "del-fs").await;
    let access = issue_access(&db, tid, uid).await;

    // Seed on-disk content under <data_root>/objects/t/<tenant>/: a wiki .md body + a capture .jsonl.
    let tenant_dir = std::path::Path::new(&data_root)
        .join("objects")
        .join("t")
        .join(tid.to_string());
    let vault = tenant_dir.join("u").join("_");
    std::fs::create_dir_all(&vault).unwrap();
    std::fs::write(vault.join("rust.md"), "# rust\nthe user's note body").unwrap();
    let caps = tenant_dir.join("u").join(uid.to_string()).join("captures");
    std::fs::create_dir_all(&caps).unwrap();
    std::fs::write(caps.join("x.jsonl"), "{\"body\":\"a capture\"}\n").unwrap();
    assert!(tenant_dir.exists(), "seeded on-disk content exists pre-delete");

    let res = app
        .clone()
        .oneshot(
            Request::delete("/v1/account")
                .header("authorization", format!("Bearer {access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "account delete → 200");

    assert!(
        !tenant_dir.exists(),
        "the tenant's on-disk vault + captures were purged from disk"
    );
}

#[sqlx::test(migrations = "../migrations")]
async fn delete_account_requires_auth(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, _uid) = seed_tenant(&db, "del-noauth").await;

    // No Authorization header → the TenantCtx extractor rejects with 401 (route exists, so it is
    // NOT a 404). A ?token= query param is also rejected on this non-SSE route.
    let res = app
        .clone()
        .oneshot(Request::delete("/v1/account").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "unauthenticated account delete → 401");

    // And nothing was deleted.
    let still: Option<Uuid> = sqlx::query_scalar("SELECT id FROM tenants WHERE id=$1")
        .bind(tid)
        .fetch_optional(&db.migrator)
        .await
        .unwrap();
    assert!(still.is_some(), "tenant untouched by an unauthenticated request");
}
