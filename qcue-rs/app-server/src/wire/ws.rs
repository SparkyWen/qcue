//! QCue v0.1.1 — the WSS JSON-RPC-lite turn transport (S3-R33/R34/R36/R40).
//!
//! Mounts `GET /v1/thread/{thread}/ws`: the client upgrades, sends `initialize`, then `turn/start`
//! (and `turn/steer`/`turn/interrupt`). The per-connection `Dispatcher` enforces the init gate
//! (`-32010`), the repeat-init reject (`-32011`), and the bounded-inflight backpressure (`-32001`); the
//! `Engine` streams the per-turn taxonomy (`turnStarted → itemStarted → itemDelta* → itemCompleted →
//! turnCompleted`) as `RuntimeEventEnvelope` frames. `turn/interrupt` cancels the in-flight turn, which
//! ends `turnCompleted{status:"interrupted"}` (pitfall #9 — a CancellationToken, never an FD dance).
//!
//! The session logic ([`run_session`]) is written against abstract inbound/outbound frame channels so it
//! is testable without a socket; [`thread_ws`] is the thin adapter that pumps a real `WebSocket` into
//! those channels (a reader task + a single serializing writer task → one writer per connection).
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use crate::wire::dispatch::{DispatchResult, Dispatcher};
use crate::wire::engine::{RecallTurnStarter, TurnStarter};
use std::sync::Arc;
use app_server_protocol::envelope::{RpcErrorBody, RpcResponse};
use app_server_protocol::{Message as RpcMessage, RpcError};
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Per-connection inflight budget (the `capacity+1`-th concurrent request → `-32001`).
const WS_INFLIGHT_CAP: usize = 8;

/// `GET /v1/thread/{thread}/ws` — authenticate (Bearer or `?token=` for browser sockets), upgrade, and
/// run the JSON-RPC-lite session. `_thread` scopes the stream (reserved for the per-Thread hub join).
pub async fn thread_ws(
    State(st): State<AppState>,
    Path(thread): Path<Uuid>,
    ctx: TenantCtx, // rejects an unauthenticated upgrade (401) before on_upgrade
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // The live turn channel runs a REAL agentic turn for THIS tenant/thread (the same open recall prompt
    // + tools the SSE recall driver uses). The client never sends tenant_id — RLS + the JWT own isolation.
    let starter: Arc<dyn TurnStarter> = Arc::new(RecallTurnStarter {
        st,
        tenant: ctx.tenant_id,
        user: ctx.user_id,
        thread,
        prefer_wiki: false,
    });
    ws.on_upgrade(move |socket| handle_socket(socket, starter))
}

/// Pump a real `WebSocket` into the abstract frame channels [`run_session`] speaks, with one reader task
/// (socket → inbound) and one writer task (outbound → socket; the single serializing writer per conn).
async fn handle_socket(socket: WebSocket, starter: Arc<dyn TurnStarter>) {
    let (mut sink, mut stream) = socket.split();
    let (in_tx, in_rx) = mpsc::channel::<String>(64);
    let (out_tx, mut out_rx) = mpsc::channel::<String>(256);

    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                WsMessage::Text(t) => {
                    if in_tx.send(t.to_string()).await.is_err() {
                        break;
                    }
                }
                WsMessage::Close(_) => break,
                _ => {} // ping/pong handled by axum; binary ignored (text JSON-RPC-lite only)
            }
        }
    });
    let writer = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            if sink.send(WsMessage::Text(frame.into())).await.is_err() {
                break;
            }
        }
    });

    run_session(in_rx, out_tx, starter).await;
    reader.abort();
    writer.abort();
}

/// The testable session core. Reads JSON-RPC-lite frames from `inbound`, routes them through the
/// `Dispatcher` + `Engine`, and writes response/error/event frames to `outbound`. A `turn/start` streams
/// the turn taxonomy on its own task (so a concurrent `turn/interrupt` can cancel it); the inflight slot
/// it took is freed when that turn ends.
pub async fn run_session(
    mut inbound: mpsc::Receiver<String>,
    outbound: mpsc::Sender<String>,
    starter: Arc<dyn TurnStarter>,
) {
    let mut dispatcher = Dispatcher::new(WS_INFLIGHT_CAP);
    let mut current_cancel: Option<CancellationToken> = None;
    // A turn task signals here when it ends, so the loop frees that turn's inflight slot.
    let (done_tx, mut done_rx) = mpsc::channel::<()>(WS_INFLIGHT_CAP + 1);

    loop {
        tokio::select! {
            biased;
            Some(()) = done_rx.recv() => dispatcher.complete_one(),
            frame = inbound.recv() => {
                let Some(frame) = frame else { break };
                let (id, method) = parse_id_method(&frame);
                match dispatcher.handle(&frame) {
                    DispatchResult::Ok(result) => {
                        if let Some(id) = &id {
                            send_json(&outbound, &RpcResponse { id: id.clone(), result }).await;
                        }
                        match method.as_deref() {
                            // a long-running turn: stream on its own task; its slot frees on `done`.
                            // turn/steer currently starts a NEW turn (overwriting current_cancel) — true
                            // mid-turn steering is a follow-up; this milestone makes turn/start REAL.
                            Some("turn/start") | Some("turn/steer") => {
                                let cancel = CancellationToken::new();
                                current_cancel = Some(cancel.clone());
                                let out = outbound.clone();
                                let done = done_tx.clone();
                                let starter = starter.clone();
                                let input = parse_input(&frame);
                                tokio::spawn(async move {
                                    let mut rx = starter.start(input, cancel).await;
                                    while let Some(env) = rx.recv().await {
                                        if out.send(serde_json::to_string(&env).unwrap_or_default()).await.is_err() {
                                            break;
                                        }
                                    }
                                    let _ = done.send(()).await;
                                });
                            }
                            // cancel the in-flight turn; this request's own slot frees immediately.
                            Some("turn/interrupt") => {
                                if let Some(c) = current_cancel.take() {
                                    c.cancel();
                                }
                                dispatcher.complete_one();
                            }
                            // initialize took NO slot; any other accepted method took one → free it now.
                            Some("initialize") => {}
                            _ => dispatcher.complete_one(),
                        }
                    }
                    DispatchResult::Err(code, message) => {
                        if let Some(id) = &id {
                            send_json(&outbound, &RpcError { id: id.clone(), error: RpcErrorBody { code, message } }).await;
                        }
                    }
                    DispatchResult::Notify => {}
                }
            }
            else => break,
        }
    }
    // ending the session cancels any in-flight turn so its task stops streaming into a dropped sink.
    if let Some(c) = current_cancel.take() {
        c.cancel();
    }
}

/// Extract the user's turn input from a `turn/start`/`turn/steer` frame's `params` (the first of
/// `input`/`text`/`message`/`prompt`/`q`). Empty when absent — the open recall prompt still yields a
/// sensible turn.
fn parse_input(frame: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(frame).unwrap_or(serde_json::Value::Null);
    let params = &v["params"];
    for key in ["input", "text", "message", "prompt", "q"] {
        if let Some(s) = params.get(key).and_then(|x| x.as_str()) {
            return s.to_string();
        }
    }
    String::new()
}

/// Parse just the id + method off a frame (the dispatcher consumes the frame but doesn't surface them).
fn parse_id_method(frame: &str) -> (Option<serde_json::Value>, Option<String>) {
    match serde_json::from_str::<RpcMessage>(frame) {
        Ok(RpcMessage::Request(r)) => (Some(r.id), Some(r.method)),
        Ok(RpcMessage::Notification(n)) => (None, Some(n.method)),
        _ => (None, None),
    }
}

async fn send_json<T: Serialize>(out: &mpsc::Sender<String>, v: &T) {
    if let Ok(s) = serde_json::to_string(v) {
        let _ = out.send(s).await;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::wire::engine::StubTurnStarter;

    /// Drive `run_session` over channels and collect the next outbound frame as JSON.
    async fn next(out: &mut mpsc::Receiver<String>) -> serde_json::Value {
        let s = tokio::time::timeout(std::time::Duration::from_secs(2), out.recv())
            .await
            .expect("a frame arrives")
            .expect("channel open");
        serde_json::from_str(&s).unwrap()
    }

    #[tokio::test]
    async fn init_gate_then_turn_stream_then_interrupt() {
        let (in_tx, in_rx) = mpsc::channel::<String>(32);
        let (out_tx, mut out) = mpsc::channel::<String>(512);
        let h = tokio::spawn(run_session(in_rx, out_tx, Arc::new(StubTurnStarter)));

        // 1) a method BEFORE initialize → -32010 (the init gate).
        in_tx.send(r#"{"id":1,"method":"turn/start","params":{}}"#.into()).await.unwrap();
        let r = next(&mut out).await;
        assert_eq!(r["id"], 1);
        assert_eq!(r["error"]["code"], -32010);

        // 2) initialize → ok.
        in_tx.send(r#"{"id":2,"method":"initialize","params":{}}"#.into()).await.unwrap();
        let r = next(&mut out).await;
        assert_eq!(r["result"]["ok"], true);

        // 3) repeat initialize → -32011.
        in_tx.send(r#"{"id":99,"method":"initialize","params":{}}"#.into()).await.unwrap();
        let r = next(&mut out).await;
        assert_eq!(r["error"]["code"], -32011);

        // 4) turn/start → an accepted response, then the streamed turn taxonomy.
        in_tx.send(r#"{"id":3,"method":"turn/start","params":{}}"#.into()).await.unwrap();
        let r = next(&mut out).await;
        assert_eq!(r["id"], 3, "accepted response for the turn/start request");

        // collect events until we've seen turnStarted, itemStarted, and at least one itemDelta.
        let (mut started, mut item, mut delta) = (false, false, false);
        for _ in 0..50 {
            let e = next(&mut out).await;
            match e["event"].as_str().unwrap_or("") {
                "turnStarted" => started = true,
                "itemStarted" => item = true,
                "itemDelta" => delta = true,
                _ => {}
            }
            if started && item && delta {
                break;
            }
        }
        assert!(started && item && delta, "the turn streams turnStarted → itemStarted → itemDelta*");

        // 5) turn/interrupt → the in-flight turn ends turnCompleted{status:"interrupted"}.
        in_tx.send(r#"{"id":4,"method":"turn/interrupt","params":{}}"#.into()).await.unwrap();
        let mut interrupted = false;
        for _ in 0..200 {
            let e = next(&mut out).await;
            if e["event"] == "turnCompleted" && e["payload"]["status"] == "interrupted" {
                interrupted = true;
                break;
            }
        }
        assert!(interrupted, "turn/interrupt cancels the turn → turnCompleted{{interrupted}}");

        drop(in_tx);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), h).await;
    }
}
