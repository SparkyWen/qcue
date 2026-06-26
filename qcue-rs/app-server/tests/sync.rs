// QCue S3-R47..R50 — the CRDT sync hub: device register (per-tenant site_id), idempotent + HLC-ordered
// op push, materialize → rebuilt state, pull-since cursor, and RLS isolation of `sync_ops` across
// tenants. All against real Postgres (`#[sqlx::test]`), keyless.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::objstore::ObjStore;
use app_server::sync::materialize::{apply_unapplied, ops_since};
use app_server::sync::routes::{push_ops, register_device, SyncOp};
use sqlx::PgPool;
use std::path::PathBuf;
use store::wiki_repo::WikiRepo;
use uuid::Uuid;
use wiki::sandbox::{TenantQuota, TenantSandbox};
use wiki::write_gate::WikiWriteGate;

/// A throwaway object store for materialization tests (`idea.create` writes the canonical JSONL line,
/// so `apply_unapplied` needs one). Each test gets a fresh per-process data root.
fn test_objstore() -> ObjStore {
    ObjStore::new(&test_data_root())
}

/// A wiki write-gate over the test pool, rooted at the per-tenant vault under `data_root` (matches
/// AppState::vault_root). Task 4 materializes `wiki_page` ops through this single body-write site.
fn test_gate(db: &TestDb, tid: Uuid, data_root: &str) -> WikiWriteGate {
    let vault_root = PathBuf::from(data_root).join("objects").join(format!("t/{tid}/u/_"));
    WikiWriteGate::new(
        WikiRepo::new(db.app.clone()),
        TenantSandbox { vault_root, quota: TenantQuota::default() },
    )
}

// ── device register: distinct per-tenant site_ids; re-register is idempotent (same site_id) ───
#[sqlx::test(migrations = "../migrations")]
async fn test_device_register_site_id(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "sync-a").await;
    let mut tx = tenant_tx(&db, tid).await;
    let d1 = register_device(&mut tx, tid, uid, "ios", "phone").await.unwrap();
    let d2 = register_device(&mut tx, tid, uid, "android", "tablet").await.unwrap();
    assert_ne!(d1.site_id, d2.site_id, "distinct site_ids within a tenant");
    // re-register the same device is idempotent (same id + same site_id).
    let d1b = register_device(&mut tx, tid, uid, "ios", "phone").await.unwrap();
    assert_eq!(d1.site_id, d1b.site_id);
    assert_eq!(d1.device_id, d1b.device_id);
    tx.commit().await.unwrap();
}

// ── push is idempotent (re-send = no-op) + materialize rebuilds state in HLC order ────────────
#[sqlx::test(migrations = "../migrations")]
async fn test_sync_ops_idempotent_order_and_converge(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "sync-b").await;
    let mut tx = tenant_tx(&db, tid).await;
    let dev = register_device(&mut tx, tid, uid, "ios", "p").await.unwrap();
    let op = SyncOp {
        hlc_wall_ms: 1000,
        hlc_lamport: 1,
        site_id: dev.site_id,
        entity_kind: "wiki_page".into(),
        entity_ref: "foo".into(),
        op: serde_json::json!({ "set_title": "Foo" }),
    };
    let first = push_ops(&mut tx, tid, uid, dev.device_id, std::slice::from_ref(&op)).await.unwrap();
    assert_eq!(first.inserted, 1, "first push inserts the op");
    let again = push_ops(&mut tx, tid, uid, dev.device_id, std::slice::from_ref(&op)).await.unwrap();
    assert_eq!(again.inserted, 0, "re-sent op is a no-op conflict (B-R21)");
    // materialize wiki ops through write_gate into the canonical wiki_pages (SYNC-D3, Task 4).
    let data_root = test_data_root();
    let objstore = ObjStore::new(&data_root);
    let gate = test_gate(&db, tid, &data_root);
    apply_unapplied(&mut tx, tid, uid, &objstore, &gate).await.unwrap();
    let title: String =
        sqlx::query_scalar("SELECT title FROM wiki_pages WHERE tenant_id=$1 AND slug=$2 AND deleted_at IS NULL")
            .bind(tid)
            .bind("foo")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(title, "Foo");
    // re-applying is a no-op (the op is already `applied`).
    let n = apply_unapplied(&mut tx, tid, uid, &objstore, &gate).await.unwrap();
    assert_eq!(n, 0, "no unapplied ops remain — materialize is idempotent");
    tx.commit().await.unwrap();
}

// ── LWW: a later HLC op overwrites an earlier scalar field (deterministic convergence) ────────
#[sqlx::test(migrations = "../migrations")]
async fn test_sync_lww_last_writer_wins(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "sync-lww").await;
    let mut tx = tenant_tx(&db, tid).await;
    let dev = register_device(&mut tx, tid, uid, "ios", "p").await.unwrap();
    let mk = |lamport: i64, title: &str| SyncOp {
        hlc_wall_ms: 1000 + lamport,
        hlc_lamport: lamport,
        site_id: dev.site_id,
        entity_kind: "wiki_page".into(),
        entity_ref: "bar".into(),
        op: serde_json::json!({ "set_title": title }),
    };
    // push the LATER op first; the materializer still applies in HLC order, so "Second" wins.
    push_ops(&mut tx, tid, uid, dev.device_id, &[mk(2, "Second")]).await.unwrap();
    push_ops(&mut tx, tid, uid, dev.device_id, &[mk(1, "First")]).await.unwrap();
    let data_root = test_data_root();
    let gate = test_gate(&db, tid, &data_root);
    apply_unapplied(&mut tx, tid, uid, &ObjStore::new(&data_root), &gate).await.unwrap();
    let title: String =
        sqlx::query_scalar("SELECT title FROM wiki_pages WHERE tenant_id=$1 AND slug=$2 AND deleted_at IS NULL")
            .bind(tid)
            .bind("bar")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(title, "Second", "last writer in HLC order wins (LWW)");
    tx.commit().await.unwrap();
}

// ── SYNC-D1 §5: an idea.create op materializes a row into the canonical `ideas` table, idempotently ─
#[sqlx::test(migrations = "../migrations")]
async fn idea_create_op_materializes_into_ideas(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "sync-idea").await;
    let objstore = test_objstore();
    let gate = test_gate(&db, tid, &test_data_root());
    let mut tx = tenant_tx(&db, tid).await;
    let dev = register_device(&mut tx, tid, uid, "ios", "p").await.unwrap();
    let op = SyncOp {
        hlc_wall_ms: 1,
        hlc_lamport: 1,
        site_id: dev.site_id,
        entity_kind: "idea".into(),
        entity_ref: "uuid-aaa".into(),
        op: serde_json::json!({ "create": { "body": "hello", "origin": "text",
            "captured_at": "2026-06-15T00:00:00Z" } }),
    };
    push_ops(&mut tx, tid, uid, dev.device_id, std::slice::from_ref(&op)).await.unwrap();
    apply_unapplied(&mut tx, tid, uid, &objstore, &gate).await.unwrap();

    // the canonical row exists, keyed by the client uuid (idempotency_key = entity_ref).
    let body: String = sqlx::query_scalar("SELECT body FROM ideas WHERE tenant_id=$1 AND idempotency_key=$2")
        .bind(tid)
        .bind("uuid-aaa")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(body, "hello");
    let origin: String = sqlx::query_scalar("SELECT origin FROM ideas WHERE tenant_id=$1 AND idempotency_key=$2")
        .bind(tid)
        .bind("uuid-aaa")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(origin, "text");
    // log_ref is NOT NULL (object-store JSONL key) — the object store wrote it.
    let log_ref: String = sqlx::query_scalar("SELECT log_ref FROM ideas WHERE tenant_id=$1 AND idempotency_key=$2")
        .bind(tid)
        .bind("uuid-aaa")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(!log_ref.is_empty(), "log_ref set from the object store");

    // re-pushing + re-applying the SAME op is idempotent: still exactly one row (ON CONFLICT DO NOTHING).
    push_ops(&mut tx, tid, uid, dev.device_id, std::slice::from_ref(&op)).await.unwrap();
    apply_unapplied(&mut tx, tid, uid, &objstore, &gate).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ideas WHERE tenant_id=$1 AND idempotency_key=$2")
        .bind(tid)
        .bind("uuid-aaa")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 1, "idea.create is idempotent by client uuid");
    tx.commit().await.unwrap();
}

// ── Task 8: idea.update / idea.delete materialize into the canonical `ideas` row (multi-device
//    parity with the HTTP PATCH/DELETE): an `update` op rewrites the body (content-compare rule) and
//    a `delete` op soft-deletes — both located by the cross-device key (idempotency_key = entity_ref) ─
#[sqlx::test(migrations = "../migrations")]
async fn idea_update_and_delete_ops_materialize_into_ideas(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "sync-idea-mut").await;
    let objstore = test_objstore();
    let gate = test_gate(&db, tid, &test_data_root());
    let mut tx = tenant_tx(&db, tid).await;
    let dev = register_device(&mut tx, tid, uid, "ios", "p").await.unwrap();

    // Seed a materialized idea whose idempotency_key is the cross-device origin ref (what a prior
    // `idea.create` op binds), so the update/delete materializers can locate it.
    let entity_ref = "mut-ref-1";
    let local = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO ideas(id,tenant_id,user_id,kind,body,log_ref,origin,idempotency_key) \
         VALUES ($1,$2,$3,'text'::idea_kind,'old','captures/x.jsonl','capture',$4)",
    )
    .bind(local)
    .bind(tid)
    .bind(uid)
    .bind(entity_ref)
    .execute(&mut *tx)
    .await
    .unwrap();

    // Push an `update` (new body) then a `delete` op for that same entity_ref, then materialize.
    let update_op = SyncOp {
        hlc_wall_ms: 1000,
        hlc_lamport: 1,
        site_id: dev.site_id,
        entity_kind: "idea".into(),
        entity_ref: entity_ref.into(),
        op: serde_json::json!({ "update": { "body": "newer" } }),
    };
    let delete_op = SyncOp {
        hlc_wall_ms: 1001,
        hlc_lamport: 2,
        site_id: dev.site_id,
        entity_kind: "idea".into(),
        entity_ref: entity_ref.into(),
        op: serde_json::json!({ "delete": {} }),
    };
    push_ops(&mut tx, tid, uid, dev.device_id, std::slice::from_ref(&update_op)).await.unwrap();
    push_ops(&mut tx, tid, uid, dev.device_id, std::slice::from_ref(&delete_op)).await.unwrap();
    apply_unapplied(&mut tx, tid, uid, &objstore, &gate).await.unwrap();

    // the update rewrote the body and the delete soft-deleted the row (located by idempotency_key).
    let (body, active): (String, bool) =
        sqlx::query_as("SELECT body, active FROM ideas WHERE tenant_id=$1 AND idempotency_key=$2")
            .bind(tid)
            .bind(entity_ref)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(body, "newer", "idea.update rewrote the body");
    assert!(!active, "idea.delete soft-deleted the row");
    tx.commit().await.unwrap();
}

// ── a LOCATION-ONLY idea.update op (NO body) converges: it must apply the co-sent location WITHOUT
//    re-ingesting (the edit handler now emits `body:null` on an unchanged-body edit, so the
//    materializer takes the location-only branch instead of the dropped `body <> $2` no-op) ────────
#[sqlx::test(migrations = "../migrations")]
async fn idea_update_location_only_op_converges(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "sync-idea-loc").await;
    let objstore = test_objstore();
    let gate = test_gate(&db, tid, &test_data_root());
    let mut tx = tenant_tx(&db, tid).await;
    let dev = register_device(&mut tx, tid, uid, "ios", "p").await.unwrap();

    // Seed a materialized idea keyed by the cross-device ref, with no location yet and a fresh
    // last_ingested_at = updated_at (NOT dirty, the steady state after an ingest).
    let entity_ref = "loc-ref-1";
    let local = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO ideas(id,tenant_id,user_id,kind,body,log_ref,origin,idempotency_key, \
                last_ingested_at) \
         VALUES ($1,$2,$3,'text'::idea_kind,'unchanged','captures/x.jsonl','capture',$4, now())",
    )
    .bind(local)
    .bind(tid)
    .bind(uid)
    .bind(entity_ref)
    .execute(&mut *tx)
    .await
    .unwrap();

    // A location-only update: NO body key (matching what the edit handler now emits on a no-body-change
    // edit) but a co-sent location. The pre-fix materializer would have taken the `body <> $2` branch
    // and updated ZERO rows (dropping the location); the fixed path applies location + last_ingested_at.
    let update_op = SyncOp {
        hlc_wall_ms: 1000,
        hlc_lamport: 1,
        site_id: dev.site_id,
        entity_kind: "idea".into(),
        entity_ref: entity_ref.into(),
        op: serde_json::json!({ "update": { "lat": 9.0, "lng": 9.0 } }),
    };
    push_ops(&mut tx, tid, uid, dev.device_id, std::slice::from_ref(&update_op)).await.unwrap();
    apply_unapplied(&mut tx, tid, uid, &objstore, &gate).await.unwrap();

    // the location landed, the body is unchanged, and the row did NOT become dirty (no re-ingest):
    // updated_at <= last_ingested_at (the GREATEST(last_ingested_at, now()) bump keeps it not-dirty).
    let (lat, lng, body, dirty): (Option<f64>, Option<f64>, String, bool) = sqlx::query_as(
        "SELECT lat, lng, body, (updated_at > last_ingested_at) AS dirty \
         FROM ideas WHERE tenant_id=$1 AND idempotency_key=$2",
    )
    .bind(tid)
    .bind(entity_ref)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(lat, Some(9.0), "location-only update applied lat");
    assert_eq!(lng, Some(9.0), "location-only update applied lng");
    assert_eq!(body, "unchanged", "body is left UNCHANGED by a location-only update");
    assert!(!dirty, "a location-only update does not dirty the row (no re-ingest)");
}

// ── pull-since returns exactly the ops after the cursor, HLC-ordered ──────────────────────────
#[sqlx::test(migrations = "../migrations")]
async fn test_sync_pull_since(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "sync-c").await;
    let mut tx = tenant_tx(&db, tid).await;
    let dev = register_device(&mut tx, tid, uid, "ios", "p").await.unwrap();
    for l in 1..=5i64 {
        let op = SyncOp {
            hlc_wall_ms: 1000 + l,
            hlc_lamport: l,
            site_id: dev.site_id,
            entity_kind: "idea".into(),
            entity_ref: "x".into(),
            op: serde_json::json!({ "n": l }),
        };
        push_ops(&mut tx, tid, uid, dev.device_id, &[op]).await.unwrap();
    }
    let since = ops_since(&mut tx, tid, 1002).await.unwrap();
    assert_eq!(since.len(), 3, "pull-since returns exactly the ops after the cursor");
    // HLC-ordered.
    assert_eq!(since[0]["n"], 3);
    assert_eq!(since[2]["n"], 5);
    tx.commit().await.unwrap();
}

// ── push redacts secrets so no provider key reaches sync_ops.op (B-R11) ───────────────────────
#[sqlx::test(migrations = "../migrations")]
async fn test_sync_push_redacts_secrets(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "sync-redact").await;
    let mut tx = tenant_tx(&db, tid).await;
    let dev = register_device(&mut tx, tid, uid, "ios", "p").await.unwrap();
    let op = SyncOp {
        hlc_wall_ms: 1000,
        hlc_lamport: 1,
        site_id: dev.site_id,
        entity_kind: "wiki_page".into(),
        entity_ref: "leak".into(),
        op: serde_json::json!({ "api_key": "sk-live-deadbeefdeadbeef", "note": "Bearer sk-live-secrettoken123" }),
    };
    push_ops(&mut tx, tid, uid, dev.device_id, &[op]).await.unwrap();
    let stored: serde_json::Value =
        sqlx::query_scalar("SELECT op FROM sync_ops WHERE tenant_id=$1 AND entity_ref='leak'")
            .bind(tid)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(stored["api_key"], "[REDACTED]", "secret-keyed field redacted");
    assert!(!stored["note"].as_str().unwrap().contains("secrettoken"), "bearer token scrubbed: {stored}");
}

// ── RLS isolates sync_ops across tenants: tenant A never sees tenant B's ops ───────────────────
#[sqlx::test(migrations = "../migrations")]
async fn test_sync_ops_rls_isolation(pool: PgPool) {
    let db = from_pool(pool);
    let (ta, ua) = seed_tenant(&db, "sync-rls-a").await;
    let (tb, ub) = seed_tenant(&db, "sync-rls-b").await;
    // tenant A pushes an op.
    {
        let mut tx = tenant_tx(&db, ta).await;
        let dev = register_device(&mut tx, ta, ua, "ios", "p").await.unwrap();
        let op = SyncOp {
            hlc_wall_ms: 1000,
            hlc_lamport: 1,
            site_id: dev.site_id,
            entity_kind: "idea".into(),
            entity_ref: "a".into(),
            op: serde_json::json!({ "n": 1 }),
        };
        push_ops(&mut tx, ta, ua, dev.device_id, &[op]).await.unwrap();
        tx.commit().await.unwrap();
    }
    // tenant B pushes a different op.
    {
        let mut tx = tenant_tx(&db, tb).await;
        let dev = register_device(&mut tx, tb, ub, "ios", "p").await.unwrap();
        let op = SyncOp {
            hlc_wall_ms: 2000,
            hlc_lamport: 1,
            site_id: dev.site_id,
            entity_kind: "idea".into(),
            entity_ref: "b".into(),
            op: serde_json::json!({ "n": 2 }),
        };
        push_ops(&mut tx, tb, ub, dev.device_id, &[op]).await.unwrap();
        tx.commit().await.unwrap();
    }
    // tenant A's pull (no app-level WHERE on tenant — RLS does it) sees ONLY A's op.
    let mut tx = tenant_tx(&db, ta).await;
    let a_ops = ops_since(&mut tx, ta, 0).await.unwrap();
    let raw_count: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_ops")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(a_ops.len(), 1, "A sees only A's op");
    assert_eq!(a_ops[0]["n"], 1);
    assert_eq!(raw_count, 1, "an unscoped count under tenant A's GUC returns only A's row (RLS)");
}

// ── Task 6: a normal capture emits a server-origin idea.create op so other devices see it on an
//    incremental pull (the app never re-snapshots — only emitted ops propagate after the cold pull) ─
#[sqlx::test(migrations = "../migrations")]
async fn capture_emits_server_idea_op(pool: PgPool) {
    use tower::ServiceExt;
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "sync-cap-emit").await;
    let tok = issue_access(&db, tid, uid).await;

    // a normal capture via the public endpoint (NOT a client sync push).
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::post("/v1/capture")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"kind":"text","body":"hi from A","origin":"text"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let cap: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let idea_id = cap["idea_id"].as_str().unwrap();

    // the capture emitted exactly one server-origin idea.create op: site_id 0 (server), applied=true
    // (the canonical row already exists), entity_ref = the idea id, op carries body/origin/captured_at.
    let mut tx = tenant_tx(&db, tid).await;
    let row: (String, String, i64, bool, serde_json::Value) = sqlx::query_as(
        "SELECT entity_kind, entity_ref, site_id, applied, op FROM sync_ops \
         WHERE tenant_id=$1 AND entity_kind='idea'",
    )
    .bind(tid)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let _ = uid;
    assert_eq!(row.0, "idea");
    assert_eq!(row.1, idea_id, "entity_ref is the idea id");
    assert_eq!(row.2, 0, "server ops use site_id 0");
    assert!(row.3, "capture-path op is already applied (the row exists)");
    assert_eq!(row.4["create"]["body"], "hi from A");
    assert_eq!(row.4["create"]["origin"], "text");
    assert!(row.4["create"]["captured_at"].is_string(), "captured_at is an ISO string: {}", row.4);
}

// ── Task 7: a COLD pull (since=0) returns a SyncDelta snapshot reflecting the canonical tables ──
#[sqlx::test(migrations = "../migrations")]
async fn pull_cold_returns_snapshot(pool: PgPool) {
    use tower::ServiceExt;
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "sync-snap").await;
    let tok = issue_access(&db, tid, uid).await;
    let _ = (tid, uid);

    // a capture (idea row) ...
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::post("/v1/capture")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"kind":"text","body":"alpha","origin":"text"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);

    // ... and a wiki page (client push → materialized into wiki_pages via write_gate).
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::post("/v1/sync/register")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"platform":"ios","display_name":"phone"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let reg: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let device_id = reg["device_id"].as_str().unwrap();
    let push_body = serde_json::json!({
        "device_id": device_id,
        "ops": [{ "hlc_wall_ms": 1000, "hlc_lamport": 1, "site_id": reg["site_id"], "entity_kind": "wiki_page", "entity_ref": "note", "op": { "set_title": "Note" } }]
    });
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::post("/v1/sync/push")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(push_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);

    // cold pull → snapshot reflects the canonical tables; cursor > 0; no incremental ops.
    let res = app
        .oneshot(
            axum::http::Request::get(format!("/v1/sync/pull?since=0&token={tok}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let delta: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let snap = &delta["snapshot"];
    assert!(snap.is_object(), "cold pull returns a snapshot: {delta}");
    let ideas = snap["ideas"].as_array().unwrap();
    assert!(
        ideas.iter().any(|i| i["body"] == "alpha" && i["captured_at"].is_string()),
        "snapshot ideas reflect the captured row: {delta}"
    );
    let pages = snap["wiki_pages"].as_array().unwrap();
    assert!(
        pages.iter().any(|p| p["slug"] == "note" && p["title"] == "Note"),
        "snapshot wiki_pages reflect the materialized page: {delta}"
    );
    assert!(delta["cursor"].as_i64().unwrap() > 0, "cursor advances past the current ops: {delta}");
    assert!(
        delta["ops"].as_array().map(|a| a.is_empty()).unwrap_or(true),
        "no incremental ops on a cold pull: {delta}"
    );
}

// ── Task 7: a WARM pull (since=cursor) returns exactly the ops after the cursor (by seq), no
//    snapshot; push returns the advanced cursor; re-pulling at the cursor is stable (no dup/miss) ──
#[sqlx::test(migrations = "../migrations")]
async fn pull_warm_returns_incremental_ops_by_seq(pool: PgPool) {
    use tower::ServiceExt;
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "sync-incr").await;
    let tok = issue_access(&db, tid, uid).await;
    let _ = (tid, uid);

    // register a device.
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::post("/v1/sync/register")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"platform":"ios","display_name":"p"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let reg: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let device_id = reg["device_id"].as_str().unwrap();
    let site = reg["site_id"].clone();

    // a small helper to push one idea.create op.
    let push = |lamport: i64, eref: &str, body: &str| {
        serde_json::json!({
            "device_id": device_id,
            "ops": [{ "hlc_wall_ms": 1000 + lamport, "hlc_lamport": lamport, "site_id": site,
                "entity_kind": "idea", "entity_ref": eref,
                "op": { "create": { "body": body, "origin": "text", "captured_at": "2026-06-15T00:00:00Z" } } }]
        })
        .to_string()
    };

    // op #1, then a cold pull to learn the cursor (now > 0).
    app.clone()
        .oneshot(
            axum::http::Request::post("/v1/sync/push")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(push(1, "alpha", "alpha")))
                .unwrap(),
        )
        .await
        .unwrap();
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::get(format!("/v1/sync/pull?since=0&token={tok}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cold: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let cursor0 = cold["cursor"].as_i64().unwrap();
    assert!(cursor0 > 0, "cursor after op#1: {cold}");

    // op #2 — the push response carries the advanced cursor.
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::post("/v1/sync/push")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(push(2, "beta", "beta")))
                .unwrap(),
        )
        .await
        .unwrap();
    let pushed: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(pushed["inserted"], 1);
    let push_cursor = pushed["cursor"].as_i64().unwrap();
    assert!(push_cursor > cursor0, "push returns the advanced cursor: {pushed}");

    // warm pull since cursor0 → exactly op#2, no snapshot.
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::get(format!("/v1/sync/pull?since={cursor0}&token={tok}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let warm: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert!(warm["snapshot"].is_null(), "warm pull has no snapshot: {warm}");
    let ops = warm["ops"].as_array().unwrap();
    assert_eq!(ops.len(), 1, "exactly the op after the cursor: {warm}");
    assert_eq!(ops[0]["entity_ref"], "beta");
    assert_eq!(ops[0]["op"]["create"]["body"], "beta");
    assert_eq!(warm["cursor"].as_i64().unwrap(), push_cursor);

    // stable: re-pulling at the same cursor yields no ops, same cursor (seq cursor, not wall_ms).
    let res = app
        .oneshot(
            axum::http::Request::get(format!("/v1/sync/pull?since={push_cursor}&token={tok}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let stable: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert!(
        stable["ops"].as_array().map(|a| a.is_empty()).unwrap_or(true),
        "no dup/miss on a repeat pull: {stable}"
    );
    assert_eq!(stable["cursor"].as_i64().unwrap(), push_cursor);
}

// ── the HTTP surface: register → push → pull round-trips over the router ───────────────────────
#[sqlx::test(migrations = "../migrations")]
async fn test_sync_http_round_trip(pool: PgPool) {
    use tower::ServiceExt;
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "sync-http").await;
    let tok = issue_access(&db, tid, uid).await;

    // register
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::post("/v1/sync/register")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"platform":"ios","display_name":"phone"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let reg: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let device_id = reg["device_id"].as_str().unwrap();

    // push
    let push_body = serde_json::json!({
        "device_id": device_id,
        "ops": [{ "hlc_wall_ms": 1000, "hlc_lamport": 1, "site_id": reg["site_id"], "entity_kind": "wiki_page", "entity_ref": "p", "op": { "set_title": "Hello" } }]
    });
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::post("/v1/sync/push")
                .header("authorization", format!("Bearer {tok}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(push_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    assert!(body_string(res).await.contains("\"inserted\":1"));

    // pull-since (SSE-allowlisted GET → ?token=)
    let res = app
        .oneshot(
            axum::http::Request::get(format!("/v1/sync/pull?since=0&token={tok}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let pulled = body_string(res).await;
    assert!(
        pulled.contains("snapshot") && pulled.contains("\"slug\":\"p\"") && pulled.contains("Hello"),
        "cold pull snapshots the materialized page: {pulled}"
    );

    // the push handler materialized the wiki op through write_gate into wiki_pages.
    let mut tx = tenant_tx(&db, tid).await;
    let title: String =
        sqlx::query_scalar("SELECT title FROM wiki_pages WHERE tenant_id=$1 AND slug=$2 AND deleted_at IS NULL")
            .bind(tid)
            .bind("p")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    let _ = uid;
    assert_eq!(title, "Hello");
}

// ── Task 8: two devices of the SAME account — A pushes, B sees it on pull (snapshot, then
//    incremental by seq); the canonical tables hold the materialized rows ─────────────────────────
#[sqlx::test(migrations = "../migrations")]
async fn two_device_round_trip(pool: PgPool) {
    use tower::ServiceExt;
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "sync-2dev").await;
    let tok = issue_access(&db, tid, uid).await;
    let _ = uid;

    // register device A and device B (same tenant/account → same JWT, distinct site_ids).
    let register = |app: axum::Router, platform: &'static str| {
        let tok = tok.clone();
        async move {
            let res = app
                .oneshot(
                    axum::http::Request::post("/v1/sync/register")
                        .header("authorization", format!("Bearer {tok}"))
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(format!(
                            r#"{{"platform":"{platform}","display_name":"{platform}"}}"#
                        )))
                        .unwrap(),
                )
                .await
                .unwrap();
            let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
            v
        }
    };
    let reg_a = register(app.clone(), "ios").await;
    let reg_b = register(app.clone(), "android").await;
    assert_ne!(reg_a["site_id"], reg_b["site_id"], "A and B get distinct site_ids");
    let dev_a = reg_a["device_id"].as_str().unwrap();
    let site_a = reg_a["site_id"].clone();

    // helper: A pushes a batch of ops.
    let push = |app: axum::Router, body: String| {
        let tok = tok.clone();
        async move {
            let res = app
                .oneshot(
                    axum::http::Request::post("/v1/sync/push")
                        .header("authorization", format!("Bearer {tok}"))
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), axum::http::StatusCode::OK);
            let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
            v
        }
    };
    // helper: pull at a cursor.
    let pull = |app: axum::Router, since: i64| {
        let tok = tok.clone();
        async move {
            let res = app
                .oneshot(
                    axum::http::Request::get(format!("/v1/sync/pull?since={since}&token={tok}"))
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), axum::http::StatusCode::OK);
            let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
            v
        }
    };

    // A pushes an idea.create + a wiki_page (set_title + set_body) — both materialize on the server.
    let first = serde_json::json!({
        "device_id": dev_a,
        "ops": [
            { "hlc_wall_ms": 1001, "hlc_lamport": 1, "site_id": site_a, "entity_kind": "idea",
              "entity_ref": "note-1",
              "op": { "create": { "body": "from A", "origin": "text", "captured_at": "2026-06-15T00:00:00Z" } } },
            { "hlc_wall_ms": 1002, "hlc_lamport": 2, "site_id": site_a, "entity_kind": "wiki_page",
              "entity_ref": "page-a",
              "op": { "set_title": "Page A", "set_body": "Body of A" } }
        ]
    });
    let pr = push(app.clone(), first.to_string()).await;
    assert_eq!(pr["inserted"], 2);

    // B cold-pulls → snapshot reflects BOTH of A's changes.
    let snap_b = pull(app.clone(), 0).await;
    let s = &snap_b["snapshot"];
    assert!(s.is_object(), "B gets a snapshot: {snap_b}");
    assert!(
        s["ideas"].as_array().unwrap().iter().any(|i| i["body"] == "from A"),
        "B sees A's capture: {snap_b}"
    );
    assert!(
        s["wiki_pages"].as_array().unwrap().iter().any(|p| p["slug"] == "page-a" && p["title"] == "Page A"),
        "B sees A's wiki page: {snap_b}"
    );
    let cursor_b = snap_b["cursor"].as_i64().unwrap();

    // A pushes another op; B incrementally pulls since its cursor → exactly the new op.
    let second = serde_json::json!({
        "device_id": dev_a,
        "ops": [{ "hlc_wall_ms": 1003, "hlc_lamport": 3, "site_id": site_a, "entity_kind": "idea",
            "entity_ref": "note-2",
            "op": { "create": { "body": "second", "origin": "text", "captured_at": "2026-06-15T00:01:00Z" } } }]
    });
    push(app.clone(), second.to_string()).await;
    let delta_b = pull(app.clone(), cursor_b).await;
    assert!(delta_b["snapshot"].is_null(), "incremental pull has no snapshot: {delta_b}");
    let ops = delta_b["ops"].as_array().unwrap();
    assert_eq!(ops.len(), 1, "exactly the one op after B's cursor: {delta_b}");
    assert_eq!(ops[0]["entity_ref"], "note-2");
    assert_eq!(ops[0]["op"]["create"]["body"], "second");

    // the canonical tables hold the materialized rows.
    let mut tx = tenant_tx(&db, tid).await;
    let ideas: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ideas WHERE tenant_id=$1 AND body IN ('from A','second')",
    )
    .bind(tid)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let page_title: String = sqlx::query_scalar(
        "SELECT title FROM wiki_pages WHERE tenant_id=$1 AND slug='page-a' AND deleted_at IS NULL",
    )
    .bind(tid)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(ideas, 2, "both captures materialized into ideas");
    assert_eq!(page_title, "Page A");
}
