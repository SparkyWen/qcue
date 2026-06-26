//! QCue v0.1.1 — AGENTIC recall (Appendix A): the model authors its OWN `recall_search` and the
//! handler runs a REAL, RLS-scoped search over the tenant's captures. Two proofs against real Postgres:
//!  1. `RecallToolExec::recall_search` returns the tenant's real capture content and NEVER another
//!     tenant's (RLS isolation), with the model's pattern passed through verbatim (A-R13).
//!  2. The full turn loop (`RouterWikiLlm` with the recall tools) routes a model-authored `recall_search`
//!     tool call to the REAL handler, feeds the result back, and the model then answers — end to end.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::ingest::RouterWikiLlm;
use app_server::recall::{run_recall_stream, RecallMode};
use app_server::recall_tools::RecallToolExec;
use protocol::{ApiError, FinishReason, Message, NormalizedResponse, Role, ToolCall};
use router::dispatch::{DispatchRequest, ProviderDispatch};
use router::tools::ToolExec;
use router::turn::Harness;
use sqlx::PgPool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wiki::llm::{SystemBlocks, TenantId, WikiLlm, WikiLlmError, WikiReq, WikiResp};

/// Seed an idea (capture) row for a tenant. Mirrors the `/v1/capture` insert (RLS-bound tx).
async fn seed_idea(db: &TestDb, tid: Uuid, uid: Uuid, body: &str) -> Uuid {
    let id = Uuid::now_v7();
    let mut tx = tenant_tx(db, tid).await;
    sqlx::query(
        "INSERT INTO ideas(id,tenant_id,user_id,kind,body,log_ref,origin,ingest_state) \
         VALUES ($1,$2,$3,'text',$4,$5,'capture','pending')",
    )
    .bind(id)
    .bind(tid)
    .bind(uid)
    .bind(body)
    .bind(format!("captures/{id}.jsonl"))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

// ── Proof 1: the real handler returns THIS tenant's content and never the other tenant's (RLS) ──
#[sqlx::test(migrations = "../migrations")]
async fn recall_search_handler_returns_real_results_rls_scoped(pool: PgPool) {
    let db = from_pool(pool);
    let (a, ua) = seed_tenant(&db, "recall-a").await;
    let (b, ub) = seed_tenant(&db, "recall-b").await;
    // Both tenants have a capture matching "database migration"; only A's may appear in A's results.
    seed_idea(&db, a, ua, "We decided to use Postgres for the database migration. SENTINEL-A").await;
    seed_idea(&db, b, ub, "We decided to use Postgres for the database migration. SENTINEL-B-SECRET").await;

    let exec = RecallToolExec::new(a, db.app.clone(), std::env::temp_dir(), None, None);
    let out = exec
        .call("recall_search", r#"{"pattern":"database migration"}"#)
        .await
        .expect("recall_search runs");

    assert!(out.contains("SENTINEL-A"), "the model sees tenant A's real capture content:\n{out}");
    assert!(
        !out.contains("SENTINEL-B-SECRET"),
        "RLS isolation: tenant B's capture must NEVER leak into A's recall:\n{out}"
    );
    assert!(out.contains(':'), "results carry a file:line citation");
}

// ── a bad pattern + an unknown tool surface as recoverable errors, not panics ──────────────────
#[sqlx::test(migrations = "../migrations")]
async fn recall_tools_handle_bad_input_gracefully(pool: PgPool) {
    let db = from_pool(pool);
    let (a, _ua) = seed_tenant(&db, "recall-c").await;
    let exec = RecallToolExec::new(a, db.app.clone(), std::env::temp_dir(), None, None);

    assert!(exec.call("recall_search", "not json").await.is_err(), "invalid json → Err (surfaced to model)");
    assert!(exec.call("recall_search", r#"{"pattern":""}"#).await.is_err(), "empty pattern → Err");
    assert!(exec.call("read_page", r#"{"slug":"nope"}"#).await.unwrap().contains("not found"));
    assert!(exec.call("frobnicate", "{}").await.is_err(), "unknown tool → Err");
    // web tools are GATED: with no web client wired (web=None) they return a clear, recoverable error
    // (the model can fall back to its own knowledge), never a panic or a silent success.
    let web_err = exec.call("web_fetch", r#"{"url":"https://example.com"}"#).await.unwrap_err();
    assert!(web_err.contains("disabled"), "web_fetch off-context → recoverable error: {web_err}");
    assert!(exec.call("web_search", r#"{"query":"rust"}"#).await.is_err(), "web_search off-context → Err");
}

/// A scripted provider: returns the queued responses in order (call 0, call 1, …). Lets the test drive
/// the EXACT agentic sequence: first a `recall_search` tool call, then a final answer.
struct SeqDispatch {
    responses: Vec<NormalizedResponse>,
    next: AtomicUsize,
}
#[async_trait::async_trait]
impl ProviderDispatch for SeqDispatch {
    async fn complete(&self, _req: &DispatchRequest, _c: CancellationToken) -> Result<NormalizedResponse, ApiError> {
        let i = self.next.fetch_add(1, Ordering::SeqCst).min(self.responses.len() - 1);
        Ok(self.responses[i].clone())
    }
}
fn tool_call_response(name: &str, args: &str) -> NormalizedResponse {
    NormalizedResponse {
        content: None,
        tool_calls: Some(vec![ToolCall { id: Some("c0".into()), name: name.into(), arguments: args.into(), provider_data: None }]),
        finish_reason: FinishReason::ToolCalls,
        reasoning: None,
        usage: None,
        provider_data: None,
    }
}
fn final_response(text: &str) -> NormalizedResponse {
    NormalizedResponse {
        content: Some(text.into()),
        tool_calls: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        usage: None,
        provider_data: None,
    }
}

// ── Proof 2: the FULL agentic loop — model authors recall_search → real handler runs → model answers ──
#[sqlx::test(migrations = "../migrations")]
async fn agentic_turn_routes_model_search_to_real_handler(pool: PgPool) {
    let db = from_pool(pool);
    let (a, ua) = seed_tenant(&db, "recall-loop").await;
    seed_idea(&db, a, ua, "The migration plan is to move from MySQL to Postgres next sprint. SENTINEL-LOOP").await;

    // The model: call 1 authors a recall_search; call 2 (after the real tool result) gives the final answer.
    let harness = Harness::with_dispatch(Box::new(SeqDispatch {
        responses: vec![
            tool_call_response("recall_search", r#"{"pattern":"migration plan"}"#),
            final_response("Per your notes, the plan is to move to Postgres next sprint."),
        ],
        next: AtomicUsize::new(0),
    }));
    // The agentic recall seam: tools advertised AND executed for real against tenant A's captures.
    let llm = RouterWikiLlm::with_recall_tools(harness, db.app.clone(), std::env::temp_dir());

    let req = WikiReq {
        system: SystemBlocks { stable_prefix: "You are QCue recall.".into() },
        messages: vec![Message {
            role: Role::User,
            content: Some("What's the migration plan?".into()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            finish_reason: None,
            reasoning: None,
            provider_data: None,
            active: true,
            is_untrusted: true,
        }],
        response_format: None,
        max_tokens: 256,
        cache_breakpoint: Some(1),
        disable_thinking: true,
    };
    let resp = llm.create_message(a, req).await.expect("agentic recall turn completes");
    // The loop ran the tool call and the model's final answer came back (not budget-exhausted).
    assert_eq!(resp.content, "Per your notes, the plan is to move to Postgres next sprint.");
}

/// A `WikiLlm` that RECORDS the system prefix + user tail it is handed, then returns a canned answer.
/// Lets a test inspect EXACTLY what instructions the user-facing recall stream gives the model.
#[derive(Clone, Default)]
struct RecordingWikiLlm {
    last_system: Arc<Mutex<String>>,
    last_tail: Arc<Mutex<String>>,
    last_msg_count: Arc<Mutex<usize>>,
}
#[async_trait::async_trait]
impl WikiLlm for RecordingWikiLlm {
    async fn create_message(&self, _t: TenantId, req: WikiReq) -> Result<WikiResp, WikiLlmError> {
        *self.last_system.lock().unwrap() = req.system.stable_prefix.clone();
        let tail = req.messages.iter().rev().find_map(|m| m.content.clone()).unwrap_or_default();
        *self.last_tail.lock().unwrap() = tail;
        *self.last_msg_count.lock().unwrap() = req.messages.len();
        Ok(WikiResp {
            content: "The capital of France is Paris.\n\n## References\n- [[france]] — country page".into(),
            usage: None,
            truncated: false,
        })
    }
}

// ── Proof 3: the USER-FACING recall stream gives the model an OPEN, tool-augmented prompt — it does NOT
//    forbid general knowledge (the bug that made the assistant unable to answer anything outside the wiki). ──
#[sqlx::test(migrations = "../migrations")]
async fn recall_stream_prompt_is_open_not_wiki_only(pool: PgPool) {
    let db = from_pool(pool);
    let (a, ua) = seed_tenant(&db, "recall-open").await;

    let recorder = RecordingWikiLlm::default();
    let mut st = app_state(&db);
    st.recall_llm = Arc::new(recorder.clone());

    app_server::recall::run_recall_stream(
        &st,
        a,
        ua,
        Uuid::now_v7(),
        "What is the capital of France?",
        app_server::recall::RecallMode::Recall,
        Default::default(),
    )
    .await;

    let sys = recorder.last_system.lock().unwrap().clone();
    let lc = sys.to_lowercase();
    assert!(!lc.contains("not general knowledge"), "recall must NOT forbid general knowledge:\n{sys}");
    assert!(!sys.contains("Answer ONLY from the wiki"), "recall must not be a closed-book wiki synthesis:\n{sys}");
    assert!(lc.contains("general knowledge"), "recall prompt must GRANT the model its general knowledge:\n{sys}");
    assert!(sys.contains("recall_search"), "recall prompt must advertise the search tool so it stays agentic:\n{sys}");

    let tail = recorder.last_tail.lock().unwrap().clone();
    assert!(tail.contains("capital of France"), "the user's question must reach the message tail:\n{tail}");
}

// ── Proof 5b (review fix): a FRESH SSE turn must NOT duplicate the current question. run_recall_stream
//    builds the request BEFORE persisting the user turn, so read_session sees no prior history and the
//    question appears EXACTLY ONCE (the fenced current question), matching the WSS path (REC-R5/S1-R38). ──
#[sqlx::test(migrations = "../migrations")]
async fn fresh_sse_turn_does_not_duplicate_the_current_question(pool: PgPool) {
    let db = from_pool(pool);
    let (a, ua) = seed_tenant(&db, "recall-nodup").await;
    let recorder = RecordingWikiLlm::default();
    let mut st = app_state(&db);
    st.recall_llm = Arc::new(recorder.clone());

    app_server::recall::run_recall_stream(
        &st, a, ua, Uuid::now_v7(), "UNIQUE-SENTINEL-Q", app_server::recall::RecallMode::Recall,
        Default::default(),
    )
    .await;

    assert_eq!(
        *recorder.last_msg_count.lock().unwrap(),
        1,
        "a fresh turn must build EXACTLY one user message (the fenced current question); the user turn \
         is persisted AFTER the request is built, so it is never replayed as a duplicate prior turn"
    );
}

// ── Proof 5: a recall turn PERSISTS the user question + final assistant answer to `messages` under
//    the right tenant (RLS), keyed by the thread, with NO tool steps (REC-R1/REC-D6). ──
#[sqlx::test(migrations = "../migrations")]
async fn recall_turn_persists_user_and_assistant_to_messages(pool: PgPool) {
    use store::messages_repo::{ConversationsRepo, MessagesRepo};
    let db = from_pool(pool);
    let (a, ua) = seed_tenant(&db, "recall-persist").await;
    let st = app_state(&db); // keyless stub recall_llm → "Answer from the wiki.\n\n## References ..."
    let thread = Uuid::now_v7();

    app_server::recall::run_recall_stream(
        &st, a, ua, thread, "What did I decide about embeddings?", app_server::recall::RecallMode::Recall,
        Default::default(),
    )
    .await;

    let msgs = MessagesRepo::new(db.app.clone()).read_session(a, thread).await.unwrap();
    let roles: Vec<&str> = msgs.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, vec!["user", "assistant"], "exactly the user turn then the final assistant turn");
    assert_eq!(msgs[0].content.as_deref(), Some("What did I decide about embeddings?"));
    assert!(msgs[1].content.as_deref().unwrap().contains("Answer from the wiki"));

    // the conversation header was upserted, titled from the question.
    let convos = ConversationsRepo::new(db.app.clone()).list(a).await.unwrap();
    assert_eq!(convos.len(), 1);
    assert_eq!(convos[0].id, thread);
    assert!(convos[0].title.starts_with("What did I decide"));
}

// ── Proof 6: a CONTINUE turn loads prior turns into the message tail (untrusted), and the system
//    prefix stays byte-stable across turns (prompt cache; REC-R5/REC-D5). ──
#[sqlx::test(migrations = "../migrations")]
async fn continue_replays_history_into_tail_with_stable_prefix(pool: PgPool) {
    use store::messages_repo::MessagesRepo;
    let db = from_pool(pool);
    let (a, ua) = seed_tenant(&db, "recall-continue").await;
    let thread = Uuid::now_v7();

    // Seed a prior turn directly in `messages` (a previously-answered exchange on this thread).
    let msgs = MessagesRepo::new(db.app.clone());
    msgs.insert_user(a, ua, thread, "We use Postgres. SENTINEL-PRIOR-Q").await.unwrap();
    msgs.insert_assistant(a, ua, thread, "Noted: Postgres. SENTINEL-PRIOR-A").await.unwrap();

    // First build (with history) to capture the prefix + tail; then a fresh-thread build → same prefix.
    let req1 = app_server::recall::build_recall_request(&db.app, a, thread, "Why Postgres again?", false, "DeepSeek", "deepseek-v4-pro").await;
    let prefix1 = req1.system.stable_prefix.clone();
    // The system prefix now states the resolved model identity truthfully (the user-facing fix).
    assert!(prefix1.contains("deepseek-v4-pro") && prefix1.contains("DeepSeek"), "prefix states identity:\n{prefix1}");

    // A continue build on a thread WITHOUT prior history (a fresh thread) → same prefix bytes.
    let req_fresh = app_server::recall::build_recall_request(&db.app, a, Uuid::now_v7(), "Why Postgres again?", false, "DeepSeek", "deepseek-v4-pro").await;
    assert_eq!(prefix1, req_fresh.system.stable_prefix, "system prefix is byte-stable regardless of history (REC-D5)");

    // The continue request carries the prior turns in the message tail, before the fenced current Q.
    let tail_blob: String = req1.messages.iter().filter_map(|m| m.content.clone()).collect::<Vec<_>>().join("\n");
    assert!(tail_blob.contains("SENTINEL-PRIOR-Q"), "prior user turn replayed into the tail:\n{tail_blob}");
    assert!(tail_blob.contains("SENTINEL-PRIOR-A"), "prior assistant turn replayed into the tail:\n{tail_blob}");
    assert!(tail_blob.contains("Why Postgres again?"), "the current question is still present:\n{tail_blob}");
    // every replayed message is flagged untrusted (it goes in the tail, never the prefix; S1-R38).
    assert!(req1.messages.iter().all(|m| m.is_untrusted), "all tail messages are untrusted");
    // the LAST message is the current fenced question (role alternation: history then current).
    assert_eq!(req1.messages.last().unwrap().role, Role::User);
}

// ── Proof 4: the recall SSE stream surfaces EACH real model tool call as its own tool_call+tool_result
//    event (per-tool-call streaming), NOT one synthetic affordance whose args are the user's question. ──
#[sqlx::test(migrations = "../migrations")]
async fn agentic_recall_surfaces_each_real_tool_call_as_sse_event(pool: PgPool) {
    let db = from_pool(pool);
    let (a, ua) = seed_tenant(&db, "recall-stream").await;
    seed_idea(&db, a, ua, "The migration plan: move from MySQL to Postgres next sprint. SENTINEL-X").await;

    // The model authors TWO real recall_search calls (verbatim patterns) across two iterations, then answers.
    let harness = Harness::with_dispatch(Box::new(SeqDispatch {
        responses: vec![
            tool_call_response("recall_search", r#"{"pattern":"migration"}"#),
            tool_call_response("recall_search", r#"{"pattern":"postgres"}"#),
            final_response("Per your notes: MySQL → Postgres next sprint.\n\n## References\n- [[migration]] — the plan"),
        ],
        next: AtomicUsize::new(0),
    }));
    let mut st = app_state(&db);
    st.recall_llm = Arc::new(RouterWikiLlm::with_recall_tools(harness, db.app.clone(), std::env::temp_dir()));

    let thread = Uuid::now_v7();
    let mut rx = st.threads.subscribe(a, thread); // subscribe (tenant-scoped) BEFORE publishing (the SSE race)

    run_recall_stream(&st, a, ua, thread, "what's the migration plan?", RecallMode::Recall, Default::default())
        .await;

    // Drain everything published to the thread.
    let mut events: Vec<(String, String)> = Vec::new();
    while let Ok(env) = rx.try_recv() {
        events.push((env.event.clone(), serde_json::to_string(&env.payload).unwrap()));
    }
    let kinds: Vec<&str> = events.iter().map(|(e, _)| e.as_str()).collect();

    let tool_calls: Vec<&(String, String)> = events.iter().filter(|(e, _)| e == "tool_call").collect();
    let tool_results = events.iter().filter(|(e, _)| e == "tool_result").count();
    assert_eq!(tool_calls.len(), 2, "each REAL model tool call is its own event; got kinds: {kinds:?}");
    assert_eq!(tool_results, 2, "one tool_result per real tool call; got kinds: {kinds:?}");
    assert!(tool_calls[0].1.contains("migration"), "verbatim model args, not the question: {}", tool_calls[0].1);
    assert!(tool_calls[1].1.contains("postgres"), "verbatim model args: {}", tool_calls[1].1);
    // the OLD synthetic affordance used the user's QUESTION as the args — that must be gone.
    assert!(
        !tool_calls.iter().any(|(_, p)| p.contains("what's the migration plan")),
        "no synthetic question-as-args tool_call should remain: {tool_calls:?}"
    );
    // …and the turn still terminates cleanly with the final answer + one usage + one done.
    assert!(kinds.contains(&"message_delta"), "the final answer still streams as message_delta: {kinds:?}");
    assert_eq!(events.iter().filter(|(e, _)| e == "usage").count(), 1, "exactly one usage: {kinds:?}");
    assert_eq!(events.iter().filter(|(e, _)| e == "done").count(), 1, "exactly one done: {kinds:?}");
}
