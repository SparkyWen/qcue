//! QCue v0.1.1 — SSE replay-on-reconnect (S3-R37/R38, the previously-unwired Task 22). A client that
//! reconnects with `?since_seq=N` (or a `Last-Event-ID` header) must get the MISSED TAIL replayed from
//! the per-stream ring before the live broadcast — instead of silently losing it. Verified against a
//! seeded ring + a real HTTP GET through the full router.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::router::build_router;
use app_server_protocol::RuntimeEventEnvelope;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

/// Collect SSE chunks while they keep arriving promptly (the backfill flushes at once); stop after a
/// short idle gap so we never wait for the 15s keep-alive heartbeat.
async fn collect_briefly(res: axum::response::Response) -> String {
    use futures_util::StreamExt;
    let mut body = res.into_body().into_data_stream();
    let mut out = String::new();
    while let Ok(Some(Ok(chunk))) =
        tokio::time::timeout(std::time::Duration::from_millis(500), body.next()).await
    {
        out.push_str(&String::from_utf8_lossy(&chunk));
    }
    out
}

fn ev(stream: Uuid, seq: u64) -> RuntimeEventEnvelope {
    RuntimeEventEnvelope {
        schema_version: 1,
        thread_id: stream,
        turn_id: None,
        seq,
        event: "progress".into(),
        payload: serde_json::json!({ "n": seq }),
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn dream_reconnect_replays_missed_tail(pool: PgPool) {
    let db = from_pool(pool);
    let st = app_state(&db);
    let app = build_router(st.clone());
    let (tid, uid) = seed_tenant(&db, "rc-a").await;
    let tok = issue_access(&db, tid, uid).await;
    let job = Uuid::now_v7();

    // Seed the dream job's ring with 5 progress events (seq 1..=5), as the dream handler would publish.
    for _ in 0..5 {
        let seq = st.dream_streams.next_seq(job);
        st.dream_streams.publish(ev(job, seq));
    }

    // Reconnect with since_seq=3 → the backfill replays seq 3,4,5; 1,2 are NOT re-sent.
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::get(format!("/v1/dream/{job}/stream?token={tok}&since_seq=3"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let body = collect_briefly(res).await;

    assert!(body.contains("\"n\":3"), "replays seq 3:\n{body}");
    assert!(body.contains("\"n\":4"), "replays seq 4");
    assert!(body.contains("\"n\":5"), "replays seq 5");
    assert!(!body.contains("\"n\":1"), "does NOT replay seq 1 (before since_seq):\n{body}");
    assert!(!body.contains("\"n\":2"), "does NOT replay seq 2 (before since_seq)");
}

#[sqlx::test(migrations = "../migrations")]
async fn last_event_id_header_resumes_after_that_seq(pool: PgPool) {
    let db = from_pool(pool);
    let st = app_state(&db);
    let app = build_router(st.clone());
    let (tid, uid) = seed_tenant(&db, "rc-b").await;
    let tok = issue_access(&db, tid, uid).await;
    let job = Uuid::now_v7();
    for _ in 0..4 {
        let seq = st.dream_streams.next_seq(job);
        st.dream_streams.publish(ev(job, seq));
    }
    // Last-Event-ID: 2 means "I received up to seq 2" → resume at seq 3 (>= 3).
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::get(format!("/v1/ingest/{job}/stream?token={tok}"))
                .header("last-event-id", "2")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // ingest_stream subscribes to st.threads, not dream_streams — seed there instead.
    drop(res);
    let job2 = Uuid::now_v7();
    for _ in 0..4 {
        let seq = st.threads.next_seq(job2);
        st.threads.publish(ev(job2, seq));
    }
    let res = app
        .oneshot(
            axum::http::Request::get(format!("/v1/ingest/{job2}/stream?token={tok}"))
                .header("last-event-id", "2")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = collect_briefly(res).await;
    assert!(!body.contains("\"n\":2"), "Last-Event-ID:2 resumes AFTER seq 2:\n{body}");
    assert!(body.contains("\"n\":3") && body.contains("\"n\":4"), "replays seq 3,4:\n{body}");
}
