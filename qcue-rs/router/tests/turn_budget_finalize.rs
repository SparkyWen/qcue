#![allow(clippy::unwrap_used, clippy::expect_used)]
// BUD-R3 — a tool-looping turn under a tiny round backstop must FINALIZE (TurnResult::Final),
// never return BudgetExhausted (which no longer exists) to the caller.
use async_trait::async_trait;
use protocol::{ApiError, CanonicalUsage, FinishReason, Message, NormalizedResponse, Role, ToolCall};
use router::dispatch::{DispatchRequest, ProviderDispatch};
use router::tools::ToolDispatcher;
use router::turn::{run_turn, Budget, CostGuard, Harness, Persistence, TurnContext, TurnResult};
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

struct AllowCost;
impl CostGuard for AllowCost {
    fn check_before_call(&self) -> Result<(), String> {
        Ok(())
    }
}

struct NoPersist;
impl Persistence for NoPersist {
    fn persist_user(&self, _m: &Message) {}
    fn persist_assistant(&self, _m: &Message) {}
}

/// A dispatch that mirrors the real finalize contract: when tools ARE advertised, return a tool_call;
/// when NO tools are advertised (the finalize pass), return the final answer text.
struct FinalizeProbeDispatch;

#[async_trait]
impl ProviderDispatch for FinalizeProbeDispatch {
    async fn complete(
        &self,
        req: &DispatchRequest,
        _cancel: CancellationToken,
    ) -> Result<NormalizedResponse, ApiError> {
        if req.tools.is_empty() {
            // finalize pass — no tools advertised → answer in prose
            Ok(NormalizedResponse {
                content: Some("here is the answer".into()),
                tool_calls: None,
                finish_reason: FinishReason::Stop,
                reasoning: None,
                usage: Some(CanonicalUsage { input: 10, output: 5, ..Default::default() }),
                provider_data: None,
            })
        } else {
            // normal pass — tools advertised → keep looping with a tool call
            Ok(NormalizedResponse {
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: Some("tc1".into()),
                    name: "recall_search".into(),
                    arguments: "{}".into(),
                    provider_data: None,
                }]),
                finish_reason: FinishReason::ToolCalls,
                reasoning: None,
                usage: Some(CanonicalUsage { input: 50, output: 5, ..Default::default() }),
                provider_data: None,
            })
        }
    }
}

#[tokio::test]
async fn deep_turn_finalizes_instead_of_erroring() {
    // BUD-R3 — A stub that ALWAYS asks for a tool keeps the loop going; with a tiny round backstop
    // the turn must still FINALIZE with a real answer (TurnResult::Final), never BudgetExhausted.
    let harness = Harness::with_dispatch(Box::new(FinalizeProbeDispatch));
    let ctx = TurnContext {
        history: vec![user("explain quantum entanglement")],
        budget: Budget::tokens(1_000_000, 2), // round backstop = 2 → forces finalize
        params: Default::default(),
        persistence: Box::new(NoPersist),
        cost_guard: Box::new(AllowCost),
        tools: ToolDispatcher::echo("recall_search"),
        tenant: uuid::Uuid::nil(),
        sink: None,
    };
    let res = run_turn(&harness, ctx, CancellationToken::new()).await.unwrap();
    match res {
        TurnResult::Final { content, .. } => {
            assert_eq!(content.as_deref(), Some("here is the answer"));
        }
        other => panic!("expected Final, got {other:?}"),
    }
}
