// QCue S3 — the wiki READ surface tests: `GET /v1/wiki/pages` (index) + `GET /v1/wiki/pages/{slug}`
// (page body + backlinks). Asserts the exact JSON shape the Flutter `WikiPage` decoder expects, the
// root-confined body read, the incoming-backlink projection, the 404→null path, and RLS isolation.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

async fn get(app: &axum::Router, path: &str, tok: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::get(path)
                .header("authorization", format!("Bearer {tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

// ── index lists every non-deleted page with the WikiPage metadata shape (no body) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_wiki_index_shape(pool: PgPool) {
    let db = from_pool(pool);
    let root = test_data_root();
    let app = test_router_at(&db, &root).await;
    let (tid, uid) = seed_tenant(&db, "wiki-idx").await;
    insert_wiki_page(&db, &root, tid, "concept", "auto-dream", "Auto-Dream",
        "The nightly consolidation pass.", "## Auto-Dream\n", &["dream"], &["agent"]).await;
    insert_wiki_page(&db, &root, tid, "entity", "approvals", "Approvals",
        "The human-in-the-loop gate.", "## Approvals\n", &[], &["safety"]).await;
    let tok = issue_access(&db, tid, uid).await;
    let res = get(&app, "/v1/wiki/pages", &tok).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    let pages = v["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 2);
    let p = &pages[0];
    // every field the Dart WikiPage decoder requires must be present + correctly typed.
    assert!(p["id"].is_string());
    assert!(p["type"].is_string());
    assert!(p["slug"].is_string());
    assert!(p["title"].is_string());
    assert!(p["summary"].is_string());
    assert_eq!(p["body_markdown"], "", "the index projection omits the body");
    assert!(p["updated"].is_string());
    assert!(p["aliases"].is_array() && p["tags"].is_array());
    assert!(p["backlinks"].is_array());
    // the wire token for an entity page is verbatim 'entity' (matches the Dart _wptWire map).
    let slugs: Vec<_> = pages.iter().map(|x| x["slug"].as_str().unwrap()).collect();
    assert!(slugs.contains(&"auto-dream") && slugs.contains(&"approvals"));
}

// ── page returns the body (root-confined read) + incoming backlinks ──
#[sqlx::test(migrations = "../migrations")]
async fn test_wiki_page_body_and_backlinks(pool: PgPool) {
    let db = from_pool(pool);
    let root = test_data_root();
    let app = test_router_at(&db, &root).await;
    let (tid, uid) = seed_tenant(&db, "wiki-pg").await;
    let target = insert_wiki_page(&db, &root, tid, "concept", "recall-architecture",
        "Recall Architecture", "How recall answers.", "## Recall Architecture\n\nGrep, not embeddings.\n",
        &["recall"], &["architecture"]).await;
    let src = insert_wiki_page(&db, &root, tid, "concept", "auto-dream", "Auto-Dream",
        "Nightly pass.", "Links to [[recall-architecture]].\n", &[], &[]).await;
    // src → target (so target gets a backlink from Auto-Dream).
    insert_wiki_link(&db, tid, src, target, "recall-architecture").await;
    let tok = issue_access(&db, tid, uid).await;
    let res = get(&app, "/v1/wiki/pages/recall-architecture", &tok).await;
    assert_eq!(res.status(), StatusCode::OK);
    let p: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(p["slug"], "recall-architecture");
    assert_eq!(p["type"], "concept");
    assert!(p["body_markdown"].as_str().unwrap().contains("Grep, not embeddings"),
        "the markdown body is read root-confined from the vault");
    let bl = p["backlinks"].as_array().unwrap();
    assert_eq!(bl.len(), 1, "one incoming link from Auto-Dream");
    // the WikiLink Dart decoder needs target_slug (+ optional target_page_id/display).
    assert_eq!(bl[0]["target_slug"], "auto-dream");
    assert!(bl[0]["target_page_id"].is_string());
    assert_eq!(bl[0]["display"], "Auto-Dream");
}

// ── page resolves by ALIAS too (the index-first selector is alias-aware) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_wiki_page_by_alias(pool: PgPool) {
    let db = from_pool(pool);
    let root = test_data_root();
    let app = test_router_at(&db, &root).await;
    let (tid, uid) = seed_tenant(&db, "wiki-alias").await;
    insert_wiki_page(&db, &root, tid, "entity", "tsinghua-university", "Tsinghua University",
        "A university.", "## Tsinghua\n", &["thu"], &[]).await;
    let tok = issue_access(&db, tid, uid).await;
    let res = get(&app, "/v1/wiki/pages/thu", &tok).await;
    assert_eq!(res.status(), StatusCode::OK);
    let p: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(p["slug"], "tsinghua-university");
}

// ── a missing slug is 404 (the Dart client maps it to null) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_wiki_page_missing_404(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "wiki-404").await;
    let tok = issue_access(&db, tid, uid).await;
    let res = get(&app, "/v1/wiki/pages/no-such-page", &tok).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// ── RLS: tenant A cannot read tenant B's pages (GUC isolation, not an app filter) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_wiki_tenant_isolation(pool: PgPool) {
    let db = from_pool(pool);
    let root = test_data_root();
    let app = test_router_at(&db, &root).await;
    let (a, ua) = seed_tenant(&db, "wiki-iso-a").await;
    let (b, _ub) = seed_tenant(&db, "wiki-iso-b").await;
    insert_wiki_page(&db, &root, b, "concept", "secret-b", "Secret B",
        "B's only page.", "## Secret B\n", &[], &[]).await;
    let tok = issue_access(&db, a, ua).await; // A's token
    // A's index is empty (B's page is invisible).
    let res = get(&app, "/v1/wiki/pages", &tok).await;
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(v["pages"].as_array().unwrap().len(), 0, "RLS hides B's page from A's index");
    // and a direct read of B's slug 404s for A.
    let res = get(&app, "/v1/wiki/pages/secret-b", &tok).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "RLS makes B's slug a 404 for A");
}
