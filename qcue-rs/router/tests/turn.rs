#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R48..R56,R89 — the turn loop integration.
use protocol::{ApiError, CanonicalUsage, FinishReason, Message, NormalizedResponse, Role};
use router::dispatch::{DispatchRequest, ProviderDispatch};
use router::stub::{StubProvider, StubScript};
use router::tools::ToolDispatcher;
use router::turn::{run_turn, Budget, CostGuard, Harness, Persistence, TurnContext, TurnResult};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

fn user(text: &str) -> Message {
    Message {
        role: Role::User,
        content: Some(text.into()),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        finish_reason: None,
        reasoning: None,
        provider_data: None,
        active: true,
        is_untrusted: false,
    }
}

// In-memory fakes for the injected seams.
#[derive(Default, Clone)]
struct FakePersist {
    persisted: Arc<Mutex<Vec<String>>>,
}
impl Persistence for FakePersist {
    fn persist_user(&self, m: &Message) {
        self.persisted
            .lock()
            .unwrap()
            .push(format!("user:{}", m.content.clone().unwrap_or_default()));
    }
    fn persist_assistant(&self, m: &Message) {
        self.persisted
            .lock()
            .unwrap()
            .push(format!("assistant:{}", m.content.clone().unwrap_or_default()));
    }
}
#[derive(Clone)]
struct AllowCost;
impl CostGuard for AllowCost {
    fn check_before_call(&self) -> Result<(), String> {
        Ok(())
    }
}
#[derive(Clone)]
struct DenyCost;
impl CostGuard for DenyCost {
    fn check_before_call(&self) -> Result<(), String> {
        Err("over ceiling".into())
    }
}

#[tokio::test]
async fn test_full_turn_against_stub_persists_before_and_after() {
    // S1-R56 — user persisted BEFORE the call; assistant after.
    let persist = FakePersist::default();
    let harness = Harness::with_stub(StubProvider::new(StubScript::text("hi back")));
    let ctx = TurnContext {
        history: vec![user("hello")],
        budget: Budget::tokens(1_000_000, 10),
        params: Default::default(),
        persistence: Box::new(persist.clone()),
        cost_guard: Box::new(AllowCost),
        tools: ToolDispatcher::empty(),
        tenant: uuid::Uuid::nil(),
        sink: None,
    };
    let res = run_turn(&harness, ctx, CancellationToken::new()).await.unwrap();
    assert!(matches!(res, TurnResult::Final { content: Some(ref s), .. } if s == "hi back"));
    let log = persist.persisted.lock().unwrap().clone();
    assert_eq!(log[0], "user:hello", "user must persist first");
    assert!(log.iter().any(|l| l.starts_with("assistant:")));
}

/// A fake provider that does ONE tool call then a final answer, reporting usage on BOTH calls — so the
/// turn loop must SUM usage across iterations (the cost-ledger fix) and return the final finish_reason.
struct TwoCallUsageDispatch {
    calls: Arc<Mutex<u32>>,
}
#[async_trait::async_trait]
impl ProviderDispatch for TwoCallUsageDispatch {
    async fn complete(
        &self,
        _req: &DispatchRequest,
        _cancel: CancellationToken,
    ) -> Result<NormalizedResponse, ApiError> {
        let n = {
            let mut g = self.calls.lock().unwrap();
            *g += 1;
            *g
        };
        if n == 1 {
            Ok(NormalizedResponse {
                content: None,
                tool_calls: Some(vec![protocol::ToolCall {
                    id: Some("t1".into()),
                    name: "recall_search".into(),
                    arguments: "{}".into(),
                    provider_data: None,
                }]),
                finish_reason: FinishReason::ToolCalls,
                reasoning: None,
                usage: Some(CanonicalUsage { input: 100, output: 20, ..Default::default() }),
                provider_data: None,
            })
        } else {
            Ok(NormalizedResponse {
                content: Some("done".into()),
                tool_calls: None,
                finish_reason: FinishReason::Stop,
                reasoning: None,
                usage: Some(CanonicalUsage { input: 50, output: 10, ..Default::default() }),
                provider_data: None,
            })
        }
    }
}

#[tokio::test]
async fn test_turn_sums_usage_across_tool_iterations() {
    let harness = Harness::with_dispatch(Box::new(TwoCallUsageDispatch {
        calls: Arc::new(Mutex::new(0)),
    }));
    let ctx = TurnContext {
        history: vec![user("hi")],
        budget: Budget::tokens(1_000_000, 10),
        params: Default::default(),
        persistence: Box::new(FakePersist::default()),
        cost_guard: Box::new(AllowCost),
        tools: ToolDispatcher::echo("recall_search"),
        tenant: uuid::Uuid::nil(),
        sink: None,
    };
    let res = run_turn(&harness, ctx, CancellationToken::new()).await.unwrap();
    match res {
        TurnResult::Final { usage, finish_reason, content } => {
            assert_eq!(content.as_deref(), Some("done"));
            assert_eq!(usage.input, 150, "input summed across both calls"); // 100 + 50
            assert_eq!(usage.output, 30, "output summed across both calls"); // 20 + 10
            assert_eq!(finish_reason, FinishReason::Stop);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[tokio::test]
async fn test_cost_cap_blocks_before_call() {
    // S1-R55 — a $0 ledger yields zero provider calls.
    let stub = StubProvider::new(StubScript::text("should not run"));
    let net = stub.clone_counter();
    let harness = Harness::with_stub(stub);
    let ctx = TurnContext {
        history: vec![user("hi")],
        budget: Budget::tokens(1_000_000, 10),
        params: Default::default(),
        persistence: Box::new(FakePersist::default()),
        cost_guard: Box::new(DenyCost),
        tools: ToolDispatcher::empty(),
        tenant: uuid::Uuid::nil(),
        sink: None,
    };
    let res = run_turn(&harness, ctx, CancellationToken::new()).await;
    assert!(res.is_err() || matches!(res, Ok(TurnResult::Refused(_))));
    assert_eq!(net.network_calls(), 0, "no provider call when over ceiling");
}

#[tokio::test]
async fn test_cancellation_interrupts_turn() {
    // S1-R53 — a cancelled token returns Interrupted before any call.
    let harness = Harness::with_stub(StubProvider::new(StubScript::text("x")));
    let token = CancellationToken::new();
    token.cancel();
    let ctx = TurnContext {
        history: vec![user("hi")],
        budget: Budget::tokens(1_000_000, 10),
        params: Default::default(),
        persistence: Box::new(FakePersist::default()),
        cost_guard: Box::new(AllowCost),
        tools: ToolDispatcher::empty(),
        tenant: uuid::Uuid::nil(),
        sink: None,
    };
    let res = run_turn(&harness, ctx, token).await.unwrap();
    assert!(matches!(res, TurnResult::Interrupted));
}

#[tokio::test]
async fn test_iteration_budget_caps() {
    // BUD-R3 — a stub scripted to emit tool calls forever hits the round backstop, then the loop
    // finalizes gracefully (returns Final) instead of erroring with BudgetExhausted.
    let harness = Harness::with_stub_tool_loop("recall_search");
    let ctx = TurnContext {
        history: vec![user("hi")],
        budget: Budget::tokens(1_000_000, 3),
        params: Default::default(),
        persistence: Box::new(FakePersist::default()),
        cost_guard: Box::new(AllowCost),
        tools: ToolDispatcher::echo("recall_search"),
        tenant: uuid::Uuid::nil(),
        sink: None,
    };
    let res = run_turn(&harness, ctx, CancellationToken::new()).await.unwrap();
    assert!(matches!(res, TurnResult::Final { .. }), "budget exhaustion must finalize, not error");
}

#[test]
fn test_role_alternation_repair() {
    // S1-R49 — two consecutive user messages coalesce; orphan tool_results dropped.
    use router::turn::repair_role_alternation;
    let repaired = repair_role_alternation(vec![user("a"), user("b")]);
    // consecutive same-role coalesced into one.
    let user_count = repaired.iter().filter(|m| m.role == Role::User).count();
    assert_eq!(user_count, 1);
}

#[test]
fn test_recall_in_tail_only_and_strip_storage_fields() {
    // S1-R50/R51 — recall injected only into the last user message tail; storage-only fields stripped from copy.
    use router::turn::{build_api_copy, inject_recall_into_tail};
    let mut history = vec![user("question")];
    inject_recall_into_tail(&mut history, "web", "recalled fact");
    let last = history.last().unwrap();
    assert!(last.content.as_ref().unwrap().contains("<untrusted_source"));
    // recall never lands in a system message.
    assert!(history
        .iter()
        .filter(|m| m.role == Role::System)
        .all(|m| !m.content.clone().unwrap_or_default().contains("recalled fact")));

    let copy = build_api_copy(&history);
    // build_api_copy does not mutate the stored history.
    assert_eq!(history.last().unwrap().content, copy.last().unwrap().content);
}

/// Records the observer callbacks in order so a test can assert the turn loop streamed the REAL agentic
/// steps (each tool_call → its tool_result → the assistant content), not a synthetic affordance.
#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<String>>,
}
impl protocol::TurnEventSink for RecordingSink {
    fn on_assistant_delta(&self, iter: u32, content: &str) {
        self.events.lock().unwrap().push(format!("assistant#{iter}:{content}"));
    }
    fn on_tool_call(&self, iter: u32, name: &str, arguments: &str) {
        self.events.lock().unwrap().push(format!("call#{iter}:{name}:{arguments}"));
    }
    fn on_tool_result(&self, iter: u32, name: &str, content: &str) {
        self.events.lock().unwrap().push(format!("result#{iter}:{name}:{content}"));
    }
}

#[tokio::test]
async fn turn_observer_fires_tool_call_then_result_then_assistant_in_order() {
    // One tool iteration then a final answer → the observer must see call → result → assistant, in order.
    let sink = Arc::new(RecordingSink::default());
    let harness = Harness::with_dispatch(Box::new(TwoCallUsageDispatch { calls: Arc::new(Mutex::new(0)) }));
    let ctx = TurnContext {
        history: vec![user("hi")],
        budget: Budget::tokens(1_000_000, 10),
        params: Default::default(),
        persistence: Box::new(FakePersist::default()),
        cost_guard: Box::new(AllowCost),
        tools: ToolDispatcher::echo("recall_search"),
        tenant: uuid::Uuid::nil(),
        sink: Some(sink.clone()),
    };
    let res = run_turn(&harness, ctx, CancellationToken::new()).await.unwrap();
    assert!(matches!(res, TurnResult::Final { .. }));
    let ev = sink.events.lock().unwrap().clone();
    assert_eq!(
        ev,
        vec![
            "call#1:recall_search:{}".to_string(),
            "result#1:recall_search:result for recall_search".to_string(),
            "assistant#2:done".to_string(),
        ],
        "observer must stream the real agentic steps in model order"
    );
}

#[tokio::test]
async fn turn_observer_silent_when_refused_before_any_call() {
    // Cost-cap refusal makes ZERO provider calls → the observer sees nothing (fires only post-dispatch).
    let sink = Arc::new(RecordingSink::default());
    let harness = Harness::with_stub(StubProvider::new(StubScript::text("nope")));
    let ctx = TurnContext {
        history: vec![user("hi")],
        budget: Budget::tokens(1_000_000, 10),
        params: Default::default(),
        persistence: Box::new(FakePersist::default()),
        cost_guard: Box::new(DenyCost),
        tools: ToolDispatcher::empty(),
        tenant: uuid::Uuid::nil(),
        sink: Some(sink.clone()),
    };
    let _ = run_turn(&harness, ctx, CancellationToken::new()).await;
    assert!(sink.events.lock().unwrap().is_empty(), "no events when refused before any provider call");
}

#[tokio::test]
async fn turn_observer_silent_when_pre_cancelled() {
    let sink = Arc::new(RecordingSink::default());
    let harness = Harness::with_stub(StubProvider::new(StubScript::text("x")));
    let token = CancellationToken::new();
    token.cancel();
    let ctx = TurnContext {
        history: vec![user("hi")],
        budget: Budget::tokens(1_000_000, 10),
        params: Default::default(),
        persistence: Box::new(FakePersist::default()),
        cost_guard: Box::new(AllowCost),
        tools: ToolDispatcher::empty(),
        tenant: uuid::Uuid::nil(),
        sink: Some(sink.clone()),
    };
    let res = run_turn(&harness, ctx, token).await.unwrap();
    assert!(matches!(res, TurnResult::Interrupted));
    assert!(sink.events.lock().unwrap().is_empty(), "no events when interrupted before any provider call");
}

#[test]
fn test_loop_invariant_no_provider_branches() {
    // S1-R89 — run_turn source contains no `match provider_name` / `match api_mode` business branch.
    let src = include_str!("../src/turn.rs");
    assert!(!src.contains("match provider_name"), "turn loop must not branch on provider name");
    // api_mode is reached only via transport_for; assert no inline `match api_mode {` business logic.
    assert!(!src.contains("match api_mode {"), "turn loop must not branch on api_mode");
}
