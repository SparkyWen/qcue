// QCue — JSON responses must declare `; charset=utf-8`. axum's `Json` emits bare `application/json`,
// which latin-1-defaulting clients (the Dart `http` package) mis-decode → CJK mojibake. This pins the
// charset on the wire so every client decodes UTF-8 correctly (defense-in-depth alongside the client fix).
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

#[sqlx::test(migrations = "../migrations")]
async fn json_responses_declare_utf8_charset(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "charset").await;
    let tok = issue_access(&db, tid, uid).await;

    let res = app
        .clone()
        .oneshot(
            Request::get("/v1/settings/models/deepseek")
                .header("authorization", format!("Bearer {tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let ct = res
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.to_ascii_lowercase().contains("charset=utf-8"),
        "JSON content-type must declare utf-8, got: {ct:?}"
    );
}
