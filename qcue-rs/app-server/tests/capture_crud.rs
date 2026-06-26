//! QCue CAP / LOC capture CRUD surface (Tasks 4-7). Task 4: the live `POST /v1/capture` handler must
//! honor the client's action-time `captured_at` (`COALESCE($client, now())`) and persist the optional
//! precise location (`lat`/`lng`/`loc_accuracy_m`, LOC-R1/R3). Without it, `captured_at` silently
//! defaults to server `now()` and location is dropped — so a capture made offline at 08:30 and flushed
//! at 14:00 would land on the wrong day, and "where was I?" context is lost.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use axum::http::StatusCode;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[sqlx::test(migrations = "../migrations")]
async fn test_capture_persists_client_time_and_location(pool: PgPool) {
    // Clone the pool first: `from_pool` consumes it, but we read the `ideas` row back afterwards.
    let p = pool.clone();
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "loc").await;
    let tok = issue_access(&db, tid, uid).await;
    let body = r#"{"kind":"text","body":"at the park","origin":"capture","captured_at":"2026-06-01T08:30:00Z","lat":31.2,"lng":121.4,"loc_accuracy_m":8.0}"#;

    let res = post(&app, "/v1/capture", &tok, body).await;
    assert_eq!(res.status(), StatusCode::OK, "capture should succeed");
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let id = Uuid::parse_str(v["idea_id"].as_str().unwrap()).unwrap();

    // Read the row back. `ideas` has FORCE RLS, so bind the tenant GUC in the SAME tx as the SELECT.
    let mut tx = p.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tid.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let row = sqlx::query("SELECT captured_at, lat, lng, loc_accuracy_m FROM ideas WHERE id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(
        row.get::<chrono::DateTime<chrono::Utc>, _>("captured_at").to_rfc3339(),
        "2026-06-01T08:30:00+00:00",
        "captured_at should honor the client's action-time instant, not server now()"
    );
    assert_eq!(row.get::<Option<f64>, _>("lat"), Some(31.2), "lat persisted");
    assert_eq!(row.get::<Option<f64>, _>("lng"), Some(121.4), "lng persisted");
    assert_eq!(row.get::<Option<f32>, _>("loc_accuracy_m"), Some(8.0), "loc_accuracy_m persisted");
}

// Task 5 (CAP-R1): GET /v1/captures/{id} returns the full detail of one capture, RLS-scoped, and 404s
// for an absent (or foreign-tenant) id. `source_page_slug` is None until the capture has been ingested
// into a distilled SOURCE page (DIG-R4).
#[sqlx::test(migrations = "../migrations")]
async fn test_capture_detail_returns_fields(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "detail").await;
    let tok = issue_access(&db, tid, uid).await;
    let res = post(&app, "/v1/capture", &tok, r#"{"kind":"text","body":"hello","origin":"capture","lat":1.0,"lng":2.0}"#).await;
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let id = v["idea_id"].as_str().unwrap();
    let res = get(&app, &format!("/v1/captures/{id}"), &tok).await;
    assert_eq!(res.status(), StatusCode::OK);
    let d: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(d["body"], "hello");
    assert_eq!(d["lat"], 1.0);
    assert_eq!(d["ingest_state"], "pending");
    // a random id → 404
    let res = get(&app, &format!("/v1/captures/{}", Uuid::now_v7()), &tok).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// Task 6 (CAP-R2): PATCH /v1/captures/{id} is content-driven. A REAL body change bumps `updated_at`
// (the `ideas_touch` trigger), lifting it past `last_ingested_at` so the dirty-scan re-ingests and
// DIG-R4 updates the linked page in place. `now()` is the TRANSACTION start instant (constant within a
// tx), so the changed-body UPDATE's `updated_at = now()` outruns the older stamped `last_ingested_at`.
#[sqlx::test(migrations = "../migrations")]
async fn test_edit_changes_body_and_dirties_for_reingest(pool: PgPool) {
    // Clone first: `from_pool` consumes the pool, but we stamp + read the `ideas` row back afterwards.
    let p = pool.clone();
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "edit").await;
    let tok = issue_access(&db, tid, uid).await;
    let res = post(&app, "/v1/capture", &tok, r#"{"kind":"text","body":"old","origin":"capture"}"#).await;
    let id = serde_json::from_str::<serde_json::Value>(&body_string(res).await).unwrap()["idea_id"]
        .as_str()
        .unwrap()
        .to_string();
    // simulate "already distilled": stamp last_ingested_at in the past. `ideas` has FORCE RLS, so bind
    // the tenant GUC in the SAME tx as the stamp/read (tx-local set_config; one connection throughout).
    let mut tx = p.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tid.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE ideas SET last_ingested_at = now() - interval '1 hour', ingest_state='ingested'::ingest_state WHERE id=$1::uuid",
    )
    .bind(&id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    // (a) body change → updated_at advances past last_ingested_at (dirty for re-ingest).
    let res = patch(&app, &format!("/v1/captures/{id}"), &tok, r#"{"body":"new text"}"#).await;
    assert_eq!(res.status(), StatusCode::OK);
    let mut tx = p.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tid.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let r = sqlx::query("SELECT body, (updated_at > last_ingested_at) AS dirty FROM ideas WHERE id=$1::uuid")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(r.get::<String, _>("body"), "new text");
    assert!(r.get::<bool, _>("dirty"), "a real body change must mark the capture dirty");
}

// Task 6 (CAP-R2): an UNCHANGED body must be a true no-op for the wiki — only location updates, and
// `last_ingested_at = GREATEST(last_ingested_at, now())` keeps it == `updated_at` (both the tx-start
// instant), so the dirty-scan (`updated_at > last_ingested_at`) stays FALSE → no re-ingest, no spend.
#[sqlx::test(migrations = "../migrations")]
async fn test_edit_unchanged_body_is_noop_for_reingest(pool: PgPool) {
    let p = pool.clone();
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "edit2").await;
    let tok = issue_access(&db, tid, uid).await;
    let res = post(&app, "/v1/capture", &tok, r#"{"kind":"text","body":"same","origin":"capture"}"#).await;
    let id = serde_json::from_str::<serde_json::Value>(&body_string(res).await).unwrap()["idea_id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut tx = p.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tid.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE ideas SET last_ingested_at = now(), ingest_state='ingested'::ingest_state WHERE id=$1::uuid")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // edit with the SAME body + only a location change → must NOT become dirty.
    let res = patch(&app, &format!("/v1/captures/{id}"), &tok, r#"{"body":"same","lat":9.0,"lng":9.0}"#).await;
    assert_eq!(res.status(), StatusCode::OK);
    let mut tx = p.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tid.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let r = sqlx::query("SELECT lat, (updated_at > last_ingested_at) AS dirty FROM ideas WHERE id=$1::uuid")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(r.get::<Option<f64>, _>("lat"), Some(9.0), "location is still updated");
    assert!(!r.get::<bool, _>("dirty"), "an unchanged body must NOT trigger re-ingest");
}

// Task 7 (CAP-R3, C5): DELETE /v1/captures/{id} soft-deletes the idea (active=false) and reversibly
// cascades into the wiki — the 1:1 `source` page is soft-deleted + a `wiki_delete` audit row is written
// (requested_by='user'); a still-sourced SHARED page keeps its merged prose but drops this idea from
// `source_ids` (Auto-Dream reconciles later). All in ONE tx; an absent id is 404.
#[sqlx::test(migrations = "../migrations")]
async fn test_delete_soft_deletes_and_cascades_wiki(pool: PgPool) {
    // Clone first: `from_pool` consumes the pool, but we seed `source_ids` + read rows back afterwards.
    let p = pool.clone();
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "del").await;
    let tok = issue_access(&db, tid, uid).await;
    let res = post(&app, "/v1/capture", &tok, r#"{"kind":"text","body":"trip","origin":"capture"}"#).await;
    let id_s = serde_json::from_str::<serde_json::Value>(&body_string(res).await).unwrap()["idea_id"]
        .as_str()
        .unwrap()
        .to_string();
    let id = Uuid::parse_str(&id_s).unwrap();

    // a 1:1 `source` page produced from this idea, and a shared `concept` page with two sources.
    // `insert_wiki_page` doesn't take source_ids, so stamp them in directly after the inserts.
    let other = Uuid::now_v7();
    let root = test_data_root();
    let src = insert_wiki_page(&db, &root, tid, "source", "trip-src", "Trip", "x", "## Trip\n", &[], &[]).await;
    let shared = insert_wiki_page(&db, &root, tid, "concept", "places", "Places", "y", "## Places\n", &[], &[]).await;
    let mut tx = p.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tid.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE wiki_pages SET source_ids=ARRAY[$1]::uuid[] WHERE id=$2")
        .bind(id)
        .bind(src)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE wiki_pages SET source_ids=ARRAY[$1,$2]::uuid[] WHERE id=$3")
        .bind(id)
        .bind(other)
        .bind(shared)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let res = delete(&app, &format!("/v1/captures/{id}"), &tok).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Read everything back under a tenant-bound tx on the cloned pool (FORCE RLS).
    let mut tx = p.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tid.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    // idea hidden from the feed.
    let active: bool = sqlx::query_scalar("SELECT active FROM ideas WHERE id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(!active, "the deleted idea is hidden (active=false)");
    // the 1:1 source page is soft-deleted.
    let src_del: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM wiki_pages WHERE id=$1")
            .bind(src)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(src_del.is_some(), "the 1:1 source page is soft-deleted");
    // the shared page keeps its prose but drops the id from source_ids and stays alive.
    let shared_row = sqlx::query("SELECT deleted_at, source_ids FROM wiki_pages WHERE id=$1")
        .bind(shared)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(
        shared_row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("deleted_at").is_none(),
        "the still-sourced shared page stays alive"
    );
    let srcs: Vec<Uuid> = shared_row.get("source_ids");
    assert!(
        !srcs.contains(&id) && srcs.contains(&other),
        "this idea is dropped from the shared page's source_ids; the other source is retained"
    );
    // a reversible audit row exists.
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM approvals WHERE action='wiki_delete' AND requested_by='user'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(n >= 1, "a reversible wiki_delete audit row was written");
    tx.commit().await.unwrap();

    // a random id → 404.
    let res = delete(&app, &format!("/v1/captures/{}", Uuid::now_v7()), &tok).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "an absent id is 404");
}
