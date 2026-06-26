//! QCue v0.1.1 — the WSS turn channel over a REAL WebSocket: proves the route mounts, auth gates the
//! upgrade (`?token=`), and a JSON-RPC-lite `initialize`/`turn/start` round-trips end to end. The session
//! LOGIC is unit-tested in `wire::ws`; this is the socket + auth + upgrade glue.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use futures_util::{SinkExt, StreamExt};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message as WsMsg;
use uuid::Uuid;

/// Bind the full app router to an ephemeral port; returns the bound address.
async fn serve(db: &TestDb) -> std::net::SocketAddr {
    let app = test_router(db).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service()).await.unwrap();
    });
    addr
}

#[sqlx::test(migrations = "../migrations")]
async fn ws_upgrade_authenticates_and_round_trips(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "ws-a").await;
    let tok = issue_access(&db, tid, uid).await;
    let addr = serve(&db).await;
    let thread = Uuid::now_v7();

    // 1) an UNAUTHENTICATED upgrade (no ?token=) is rejected at the HTTP handshake (401), not upgraded.
    let unauth = tokio_tungstenite::connect_async(format!("ws://{addr}/v1/thread/{thread}/ws")).await;
    assert!(unauth.is_err(), "ws upgrade without a token must be rejected");

    // 2) an authenticated upgrade succeeds, and the JSON-RPC-lite handshake round-trips.
    let url = format!("ws://{addr}/v1/thread/{thread}/ws?token={tok}");
    let (mut ws, _resp) = tokio_tungstenite::connect_async(url).await.expect("authenticated ws upgrade");

    ws.send(WsMsg::Text(r#"{"id":1,"method":"initialize","params":{}}"#.into())).await.unwrap();
    let reply = next_text(&mut ws).await;
    let v: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(v["id"], 1);
    assert_eq!(v["result"]["ok"], true, "initialize acknowledged over the real socket");

    // 3) turn/start streams the turn taxonomy — confirm we receive turnStarted over the wire.
    ws.send(WsMsg::Text(r#"{"id":2,"method":"turn/start","params":{}}"#.into())).await.unwrap();
    let mut saw_turn_started = false;
    for _ in 0..40 {
        let t = next_text(&mut ws).await;
        if t.contains("\"turnStarted\"") {
            saw_turn_started = true;
            break;
        }
    }
    assert!(saw_turn_started, "the turn streams the turnStarted event over the real WebSocket");
    let _ = ws.close(None).await;
}

/// Read frames until a Text frame arrives (skipping ping/pong); panics on timeout.
async fn next_text<S>(ws: &mut S) -> String
where
    S: StreamExt<Item = Result<WsMsg, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    for _ in 0..200 {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(3), ws.next())
            .await
            .expect("a ws frame arrives")
            .expect("stream open")
            .expect("ws ok");
        if let WsMsg::Text(t) = msg {
            return t.to_string();
        }
    }
    panic!("no text frame")
}
