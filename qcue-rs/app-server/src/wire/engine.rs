//! QCue S3-R40/R41 — the per-stream Engine actor: `Op` in (mpsc), `Event` out (mpsc), one serializing
//! writer per connection. The decoupling is the point: the dispatcher pushes `Op`s into a per-stream
//! background Tokio task that streams `RuntimeEventEnvelope`s out a *bounded* mpsc; a slow client that
//! stops draining its receiver can only back-pressure ITS OWN stream — it never blocks the dispatcher
//! or any other stream's task. Interrupt is a `CancellationToken` + dropping the in-flight future
//! (pitfall #9 — never a thread/FD dance); the turn ends with `turn/completed{status:"interrupted"}`.
//!
//! The streaming invariant (Thread→Turn→Item): a turn opens with `turn/started`, each item is
//! `item/started → delta* → item/completed`, and the turn closes with `turn/completed` — a delta is
//! NEVER emitted after that item's completion.
use crate::state::AppState;
use app_server_protocol::{RuntimeEvent, RuntimeEventEnvelope};
use protocol::TurnEventSink;
use store::messages_repo::{ConversationsRepo, MessagesRepo};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// An operation pushed INTO a stream's engine task.
pub enum Op {
    Start { input: String },
    Steer { input: String },
    Interrupt,
}

/// The terminal state of a turn: `"completed"` or `"interrupted"`.
pub struct TurnEnd {
    pub status: String,
}

/// The Engine. In production it owns the per-Thread registry + the router harness; here it is the thin
/// seam that spawns per-stream tasks. Each task is independent, so one stalled consumer can't stall
/// another (the slow-client-isolation invariant).
pub struct Engine;

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Engine
    }

    /// Start a stubbed streaming turn on its own background task; `RuntimeEventEnvelope`s flow out the
    /// returned receiver until the turn completes or `cancel` fires. The task owns its own writer, so a
    /// receiver that is never drained back-pressures only this stream.
    pub async fn start_turn_stub(
        &self,
        cancel: CancellationToken,
    ) -> mpsc::Receiver<RuntimeEventEnvelope> {
        let (tx, rx) = mpsc::channel(16);
        let thread_id = Uuid::now_v7();
        let turn_id = Uuid::now_v7();
        tokio::spawn(async move {
            let mut seq = 0u64;
            // turn/started
            if tx
                .send(env(thread_id, Some(turn_id), &mut seq, RuntimeEvent::TurnStarted, serde_json::Value::Null))
                .await
                .is_err()
            {
                return;
            }
            // one item: item/started → deltas → item/completed (the streaming invariant).
            if tx
                .send(env(thread_id, Some(turn_id), &mut seq, RuntimeEvent::ItemStarted, serde_json::json!({"type":"agentMessage"})))
                .await
                .is_err()
            {
                return;
            }
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        // close the item before the turn (invariant), then turn/completed{interrupted}.
                        let _ = tx.send(env(thread_id, Some(turn_id), &mut seq, RuntimeEvent::ItemCompleted, serde_json::Value::Null)).await;
                        let e = env(thread_id, Some(turn_id), &mut seq, RuntimeEvent::TurnCompleted, serde_json::json!({"status":"interrupted"}));
                        let _ = tx.send(e).await;
                        break;
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {
                        let delta = env(thread_id, Some(turn_id), &mut seq, RuntimeEvent::ItemDelta, serde_json::json!({"delta":"."}));
                        if tx.send(delta).await.is_err() { break; }
                    }
                }
            }
        });
        rx
    }

    /// Drain a turn's event stream to its terminal `turn/completed`, returning its status.
    pub async fn drain_to_completion(
        &self,
        rx: &mut mpsc::Receiver<RuntimeEventEnvelope>,
    ) -> TurnEnd {
        while let Some(e) = rx.recv().await {
            // `event` is the forward-compat wire String; compare against the canonical token.
            if e.event == RuntimeEvent::TurnCompleted.as_wire() {
                let status = e
                    .payload
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("completed")
                    .to_string();
                return TurnEnd { status };
            }
        }
        TurnEnd { status: "completed".into() }
    }

    /// Spawn an independent stream task (a stub consumer's stream). Used by the slow-client test.
    pub async fn spawn_thread_stub(&self) -> mpsc::Receiver<RuntimeEventEnvelope> {
        self.start_turn_stub(CancellationToken::new()).await
    }

    /// Pump up to `n` events from a stream, returning how many flowed. A different stream's stalled
    /// receiver never reduces this count (independent tasks → slow-client isolation, S3-R40).
    pub async fn pump(&self, rx: &mut mpsc::Receiver<RuntimeEventEnvelope>, n: usize) -> usize {
        let mut got = 0usize;
        while got < n {
            match rx.recv().await {
                Some(_) => got += 1,
                None => break,
            }
        }
        got
    }
}

/// How a `turn/start` becomes a stream of `RuntimeEventEnvelope`s. The session loop ([`run_session`]) is
/// generic over this so tests inject the canned [`StubTurnStarter`] (keyless, networkless) while
/// production injects [`RecallTurnStarter`] — a REAL agentic turn through `recall_llm`. `start` spawns the
/// producing task and returns the receiver immediately; events flow until completion or `cancel` fires.
#[async_trait::async_trait]
pub trait TurnStarter: Send + Sync + 'static {
    async fn start(&self, input: String, cancel: CancellationToken)
        -> mpsc::Receiver<RuntimeEventEnvelope>;
}

/// The keyless stub turn (the original canned stream): `turnStarted → itemStarted → itemDelta* → …`,
/// closing on cancel. Keeps the wire session tests networkless and byte-stable.
pub struct StubTurnStarter;

#[async_trait::async_trait]
impl TurnStarter for StubTurnStarter {
    async fn start(
        &self,
        _input: String,
        cancel: CancellationToken,
    ) -> mpsc::Receiver<RuntimeEventEnvelope> {
        Engine::new().start_turn_stub(cancel).await
    }
}

/// The REAL agentic turn: runs the OPEN recall prompt + tools through `recall_llm` (the same machinery
/// the SSE recall driver uses) and streams the Thread→Turn→Item taxonomy. Interrupt drops the in-flight
/// `create_message_observed` future (CancellationToken, pitfall #9) → `turnCompleted{interrupted}`.
pub struct RecallTurnStarter {
    pub st: AppState,
    pub tenant: Uuid,
    pub user: Uuid,
    pub thread: Uuid,
    pub prefer_wiki: bool,
}

#[async_trait::async_trait]
impl TurnStarter for RecallTurnStarter {
    async fn start(
        &self,
        input: String,
        cancel: CancellationToken,
    ) -> mpsc::Receiver<RuntimeEventEnvelope> {
        // Build the open agentic request (DB-backed index hint) BEFORE spawning, so a build failure can't
        // strand a half-open turn. The whole turn is then driven on an isolated task. Resolve the tenant's
        // real (provider, model) so the system prompt states the model's true identity (mirrors the WSS
        // recall path; the keyless stub reports a neutral identity).
        let (provider, model) = if crate::dispatch::stub_llm_enabled() {
            ("stub".to_string(), "stub".to_string())
        } else {
            crate::dispatch::effective_route(&self.st.pool, self.tenant).await
        };
        let provider_display = crate::dispatch::provider_display_name(&provider).to_string();
        let req = crate::recall::build_recall_request(
            &self.st.pool,
            self.tenant,
            self.thread,
            &input,
            self.prefer_wiki,
            &provider_display,
            &model,
        )
        .await;
        // REC-R1/REC-D6: persist the user turn + upsert the conversation header BEFORE the call.
        let pool = self.st.pool.clone();
        let (tenant, user, thread) = (self.tenant, self.user, self.thread);
        let _ = MessagesRepo::new(pool.clone()).insert_user(tenant, user, thread, &input).await;
        let _ = ConversationsRepo::new(pool.clone()).upsert(tenant, user, thread, &input).await;

        let (tx, rx) = mpsc::channel(64);
        let recall_llm = self.st.recall_llm.clone();
        let turn_id = Uuid::now_v7();
        tokio::spawn(async move {
            let seq = Arc::new(AtomicU64::new(0));
            let make = |event: RuntimeEvent, payload: serde_json::Value| {
                let s = seq.fetch_add(1, Ordering::SeqCst) + 1;
                RuntimeEventEnvelope {
                    schema_version: 1,
                    thread_id: thread,
                    turn_id: Some(turn_id),
                    seq: s,
                    event: event.as_wire().to_string(),
                    payload,
                }
            };
            if tx.send(make(RuntimeEvent::TurnStarted, serde_json::Value::Null)).await.is_err() {
                return;
            }
            // D17/B-R20 — enforce the daily cost ceiling BEFORE dispatch (mirrors the SSE recall path);
            // the in-loop wiki cost guard is a no-op, so this is the only pre-call spend gate here.
            match store::cost_repo::CostRepo::new(pool.clone()).check_ceiling(tenant, user).await {
                Ok(Ok(())) => {}
                Ok(Err(reason)) => {
                    let _ = tx.send(make(RuntimeEvent::ItemStarted, serde_json::json!({ "type": "error" }))).await;
                    let _ = tx
                        .send(make(
                            RuntimeEvent::ItemDelta,
                            serde_json::json!({ "delta": reason, "code": app_server_protocol::error_codes::COST_CEILING }),
                        ))
                        .await;
                    let _ = tx.send(make(RuntimeEvent::ItemCompleted, serde_json::json!({ "type": "error" }))).await;
                    let _ = tx.send(make(RuntimeEvent::TurnCompleted, serde_json::json!({ "status": "failed" }))).await;
                    return;
                }
                Err(e) => {
                    tracing::warn!(error = %e, %tenant, "wss recall cost-ceiling check failed");
                    let _ = tx.send(make(RuntimeEvent::ItemStarted, serde_json::json!({ "type": "error" }))).await;
                    let _ = tx
                        .send(make(RuntimeEvent::ItemDelta, serde_json::json!({ "delta": "cost check failed; try again" })))
                        .await;
                    let _ = tx.send(make(RuntimeEvent::ItemCompleted, serde_json::json!({ "type": "error" }))).await;
                    let _ = tx.send(make(RuntimeEvent::TurnCompleted, serde_json::json!({ "status": "failed" }))).await;
                    return;
                }
            }
            // Bound concurrent heavy turns (shared cap with the SSE recall driver) — over the cap → a
            // transient failed turn rather than unbounded agentic fan-out.
            let _permit = match crate::recall::recall_concurrency().try_acquire() {
                Ok(p) => p,
                Err(_) => {
                    let _ = tx.send(make(RuntimeEvent::ItemStarted, serde_json::json!({ "type": "error" }))).await;
                    let _ = tx
                        .send(make(
                            RuntimeEvent::ItemDelta,
                            serde_json::json!({ "delta": "server busy; please retry shortly", "code": app_server_protocol::error_codes::OVERLOADED }),
                        ))
                        .await;
                    let _ = tx.send(make(RuntimeEvent::ItemCompleted, serde_json::json!({ "type": "error" }))).await;
                    let _ = tx.send(make(RuntimeEvent::TurnCompleted, serde_json::json!({ "status": "failed" }))).await;
                    return;
                }
            };
            // The sink streams each model tool_call/tool_result and the answer as Items (non-blocking).
            let sink: Arc<dyn TurnEventSink> =
                Arc::new(WsTurnSink { tx: tx.clone(), thread, turn_id, seq: seq.clone() });
            let terminal = tokio::select! {
                res = recall_llm.create_message_observed(tenant, req, Some(sink)) => match res {
                    Ok(resp) => {
                        // REC-R1/REC-D6: persist the FINAL assistant text only (no tool steps).
                        let _ = MessagesRepo::new(pool.clone()).insert_assistant(tenant, user, thread, &resp.content).await;
                        make(RuntimeEvent::TurnCompleted, serde_json::json!({ "status": "completed" }))
                    }
                    Err(e) => {
                        // surface a terminal error Item, then a failed turn (never a silent dead engine).
                        let _ = tx.send(make(RuntimeEvent::ItemStarted, serde_json::json!({ "type": "error" }))).await;
                        let _ = tx.send(make(RuntimeEvent::ItemDelta, serde_json::json!({ "delta": e.to_string() }))).await;
                        let _ = tx.send(make(RuntimeEvent::ItemCompleted, serde_json::json!({ "type": "error" }))).await;
                        make(RuntimeEvent::TurnCompleted, serde_json::json!({ "status": "failed" }))
                    }
                },
                _ = cancel.cancelled() => {
                    // Dropping the create_message_observed future aborts the in-flight turn. No Item is
                    // open (each sink callback self-closes), so the streaming invariant holds.
                    make(RuntimeEvent::TurnCompleted, serde_json::json!({ "status": "interrupted" }))
                }
            };
            let _ = tx.send(terminal).await;
        });
        rx
    }
}

/// Streamed-content preview cap for tool results (the broadcast/mpsc buffer carries a readable head, not
/// an 8KB page body — the MODEL still gets the full result via the Tool message).
const WS_TOOL_RESULT_PREVIEW_CHARS: usize = 600;

/// A [`TurnEventSink`] that maps the turn loop's tool/assistant events onto the WSS Thread→Turn→Item
/// taxonomy. Each callback is a SELF-CONTAINED item (`itemStarted → [itemDelta] → itemCompleted`), so the
/// streaming invariant (no delta after an item completes) always holds and there is no item left open
/// across the provider await (so interrupt needs only emit `turnCompleted`). Pushes are non-blocking
/// (`try_send`) — the sink runs inline on the turn task, so a slow client must never stall the loop.
struct WsTurnSink {
    tx: mpsc::Sender<RuntimeEventEnvelope>,
    thread: Uuid,
    turn_id: Uuid,
    seq: Arc<AtomicU64>,
}

impl WsTurnSink {
    fn emit(&self, event: RuntimeEvent, payload: serde_json::Value) {
        let s = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.tx.try_send(RuntimeEventEnvelope {
            schema_version: 1,
            thread_id: self.thread,
            turn_id: Some(self.turn_id),
            seq: s,
            event: event.as_wire().to_string(),
            payload,
        });
    }
}

impl TurnEventSink for WsTurnSink {
    fn on_tool_call(&self, _iter: u32, name: &str, arguments: &str) {
        let args: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::Value::String(arguments.to_string()));
        self.emit(RuntimeEvent::ItemStarted, serde_json::json!({ "type": "toolCall", "name": name, "args": args }));
        self.emit(RuntimeEvent::ItemCompleted, serde_json::json!({ "type": "toolCall", "name": name }));
    }
    fn on_tool_result(&self, _iter: u32, name: &str, content: &str) {
        let preview: String = content.chars().take(WS_TOOL_RESULT_PREVIEW_CHARS).collect();
        self.emit(RuntimeEvent::ItemStarted, serde_json::json!({ "type": "toolResult", "name": name }));
        self.emit(RuntimeEvent::ItemDelta, serde_json::json!({ "delta": preview }));
        self.emit(RuntimeEvent::ItemCompleted, serde_json::json!({ "type": "toolResult", "name": name }));
    }
    fn on_assistant_delta(&self, _iter: u32, content: &str) {
        self.emit(RuntimeEvent::ItemStarted, serde_json::json!({ "type": "agentMessage" }));
        self.emit(RuntimeEvent::ItemDelta, serde_json::json!({ "delta": content }));
        self.emit(RuntimeEvent::ItemCompleted, serde_json::json!({ "type": "agentMessage" }));
    }
}

/// Build one `RuntimeEventEnvelope`, bumping `seq`. The wire `event` is the canonical camelCase token
/// for the known kind (`RuntimeEvent::as_wire`); future kinds would carry an arbitrary String.
fn env(
    thread_id: Uuid,
    turn_id: Option<Uuid>,
    seq: &mut u64,
    event: RuntimeEvent,
    payload: serde_json::Value,
) -> RuntimeEventEnvelope {
    *seq += 1;
    RuntimeEventEnvelope {
        schema_version: 1,
        thread_id,
        turn_id,
        seq: *seq,
        event: event.as_wire().to_string(),
        payload,
    }
}
