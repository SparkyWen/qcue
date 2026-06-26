// QCue — `GET /version` exposes build provenance (git SHA + build time) UNAUTHENTICATED, at the root
// (next to /healthz /readyz). This is the externally-reachable half of the stale-binary guard: a deploy
// check / drift monitor curls it with no token and asserts the live SHA == the merged SHA.
// See docs/postmortems/2026-06-17-stale-binary-incident.md.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

#[sqlx::test(migrations = "../migrations")]
async fn version_is_public_and_reports_the_build_sha(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;

    // No Authorization header — /version must be reachable without a JWT (it's a deploy/monitor probe).
    let res = app
        .oneshot(Request::get("/version").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "/version must be public (no auth)");

    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let sha = v.get("sha").and_then(|s| s.as_str()).unwrap_or("");
    assert!(
        !sha.is_empty() && sha != "unknown",
        "/version must report a real build SHA, got {sha:?}"
    );
    for k in ["short_sha", "dirty", "built_at", "pkg_version"] {
        assert!(v.get(k).is_some(), "/version JSON missing `{k}`");
    }
}
