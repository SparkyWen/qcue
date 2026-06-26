// QCue S1-R48..R56,R89 — the ONE turn loop. Provider behavior reached only via transport_for + hooks.
use crate::dispatch::ProviderDispatch;
use crate::sanitize::fence_untrusted;
use crate::stub::StubProvider;
use crate::tools::ToolDispatcher;
use protocol::{CanonicalUsage, FinishReason, Message, NormalizedResponse, Role};

pub trait Persistence: Send + Sync {
    fn persist_user(&self, m: &Message);
    fn persist_assistant(&self, m: &Message);
}
pub trait CostGuard: Send + Sync {
    /// S1-R55 / B-R20 / D17 — read the per-tenant/day (and per-user/day) ledger BEFORE any call;
    /// `Err(reason)` aborts the turn with a refusal and no provider call is made.
    fn check_before_call(&self) -> Result<(), String>;
}

/// BUD-R1..R3 — a per-turn spend budget: a TOKEN ceiling (the real cost proxy, since the growing
/// transcript is re-sent every round) plus a generous ROUND backstop so a zero-usage stub or a
/// degenerate loop still terminates. When either is hit the turn FINALIZES (one tool-less "answer now"
/// call) instead of erroring — see `run_turn`.
pub struct Budget {
    tokens_remaining: u64,
    round: u32,
    max_rounds: u32,
}
impl Budget {
    /// `tokens` = the summed input+output+reasoning+cache_write ceiling for the whole turn;
    /// `max_rounds` = the hard provider-call backstop.
    pub fn tokens(tokens: u64, max_rounds: u32) -> Self {
        Self { tokens_remaining: tokens, round: 0, max_rounds }
    }
    /// True once the token ceiling is spent OR the round backstop is reached → the loop must finalize.
    pub fn exhausted(&self) -> bool {
        self.tokens_remaining == 0 || self.round >= self.max_rounds
    }
    /// Count one completed provider round.
    pub fn tick_round(&mut self) {
        self.round = self.round.saturating_add(1);
    }
    /// Debit the tokens a provider call reported.
    pub fn consume_tokens(&mut self, n: u64) {
        self.tokens_remaining = self.tokens_remaining.saturating_sub(n);
    }
    /// The number of completed rounds (for the observer's iter label).
    pub fn round(&self) -> u32 {
        self.round
    }
}

pub struct TurnContext {
    pub history: Vec<Message>,
    pub budget: Budget,
    pub params: crate::transport::ReqParams,
    pub persistence: Box<dyn Persistence>,
    pub cost_guard: Box<dyn CostGuard>,
    pub tools: ToolDispatcher,
    /// The tenant whose credentials/pool the real dispatch resolves (S3). `nil()` where it doesn't matter.
    pub tenant: uuid::Uuid,
    /// Optional non-blocking observer: streams each model-authored tool_call/tool_result and the
    /// assistant content to a higher crate (recall SSE, WSS turn channel) as the turn runs. `None` ⇒
    /// byte-identical to no sink. The loop never branches on WHO is watching (S1-R89).
    pub sink: Option<std::sync::Arc<dyn protocol::TurnEventSink>>,
}

#[derive(Debug)]
pub enum TurnResult {
    /// The completed turn: the assistant content plus the usage accumulated across ALL provider calls
    /// in the (possibly multi-iteration tool) loop, and the final `finish_reason` (Length ⇒ truncated).
    Final {
        content: Option<String>,
        usage: CanonicalUsage,
        finish_reason: FinishReason,
    },
    Interrupted,
    Refused(String),
}

/// The harness. The loop reaches a provider ONLY through the `dispatch` seam (S1-R89); the default
/// `StubDispatch` keeps every existing test byte-identical, while `with_dispatch` injects the real
/// `HttpDispatch` (transport_for(api_mode) + pool + classify/retry) in production.
pub struct Harness {
    dispatch: Box<dyn ProviderDispatch>,
    /// Kept for compatibility with existing harness builders; the turn's round backstop in `Budget`
    /// supersedes this field — the loop no longer reads it.
    #[allow(dead_code)]
    max_iterations: u32,
    #[allow(dead_code)]
    tool_loop_tool: Option<String>,
}
impl Harness {
    pub fn with_stub(stub: StubProvider) -> Self {
        Self {
            dispatch: Box::new(crate::dispatch::StubDispatch(stub)),
            max_iterations: 16,
            tool_loop_tool: None,
        }
    }
    pub fn with_stub_tool_loop(tool: &str) -> Self {
        Self {
            dispatch: Box::new(crate::dispatch::StubDispatch(StubProvider::new(
                crate::stub::StubScript::tool_call(tool, "{}"),
            ))),
            max_iterations: 16,
            tool_loop_tool: Some(tool.into()),
        }
    }
    /// Production: inject a real (or fake) dispatch.
    pub fn with_dispatch(dispatch: Box<dyn ProviderDispatch>) -> Self {
        Self { dispatch, max_iterations: 16, tool_loop_tool: None }
    }
}

/// S1-R49 — coalesce consecutive same-role messages + drop orphan tool_results.
pub fn repair_role_alternation(msgs: Vec<Message>) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    for m in msgs {
        if let Some(last) = out.last_mut()
            && last.role == m.role
            && (m.role == Role::User || m.role == Role::Assistant)
        {
            let merged = format!(
                "{}\n{}",
                last.content.clone().unwrap_or_default(),
                m.content.clone().unwrap_or_default()
            );
            last.content = Some(merged.trim().to_string());
            continue;
        }
        out.push(m);
    }
    crate::compress::clean_orphan_tool_results(out)
}

/// S1-R50 — inject recall ONLY into the tail of the current (last) user message, fenced.
pub fn inject_recall_into_tail(history: &mut [Message], origin: &str, recall: &str) {
    if let Some(last_user) = history.iter_mut().rev().find(|m| m.role == Role::User) {
        let fenced = fence_untrusted(origin, recall);
        let base = last_user.content.clone().unwrap_or_default();
        last_user.content = Some(format!("{base}\n\n{fenced}"));
        last_user.is_untrusted = true;
    }
}

/// S1-R48 — build a sanitized COPY (role-repair applied); the stored history is never mutated here.
pub fn build_api_copy(history: &[Message]) -> Vec<Message> {
    repair_role_alternation(history.to_vec())
}

fn build_assistant_message(nr: &NormalizedResponse) -> Message {
    Message {
        role: Role::Assistant,
        content: nr.content.clone(),
        tool_call_id: None,
        tool_name: None,
        tool_calls: nr.tool_calls.clone(),
        finish_reason: Some(nr.finish_reason),
        reasoning: nr.reasoning.clone(),
        provider_data: nr.provider_data.clone(),
        active: true,
        is_untrusted: false,
    }
}

/// BUD-R1 — the per-call token cost debited from the turn budget: input + output + reasoning + the
/// (expensive) cache writes. cache_read is excluded (cheap, and re-counted every round).
fn turn_tokens(u: &CanonicalUsage) -> u64 {
    u.input + u.output + u.reasoning + u.cache_write
}

/// BUD-R3 — the one-shot "answer now" instruction appended to the message tail when the budget is spent.
/// Trusted (system-authored) guidance, so `is_untrusted: false`.
fn finalize_nudge() -> Message {
    Message {
        role: Role::User,
        content: Some(
            "You have gathered enough context. Answer the user's question now, fully, using only the \
             information already available. Do not call any tools."
                .to_string(),
        ),
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

/// The turn loop. It NEVER branches on provider name or wire mode; provider behavior is reached only
/// via transport_for(..) + profile.hooks, so the loop body never grows per provider (S1-R89).
pub async fn run_turn(
    harness: &Harness,
    mut ctx: TurnContext,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<TurnResult, String> {
    // prologue: persist the user capture BEFORE any call (S1-R56).
    if let Some(u) = ctx.history.iter().rev().find(|m| m.role == Role::User) {
        ctx.persistence.persist_user(u);
    }
    let mut messages = ctx.history.clone();
    // Accumulate usage across every provider call so the cost ledger sees the WHOLE turn (S1-R55).
    let mut total_usage = CanonicalUsage::default();
    let mut nudged = false;
    loop {
        if cancel.is_cancelled() {
            return Ok(TurnResult::Interrupted); // S1-R53 (pitfall #9)
        }
        // BUD-R3: once the token ceiling / round backstop is spent, make ONE final tool-less call that
        // forces an answer from the context already gathered — never error out and discard the work.
        let finalizing = ctx.budget.exhausted();
        if finalizing && !nudged {
            messages.push(finalize_nudge());
            nudged = true;
        }
        let api_msgs = build_api_copy(&messages); // S1-R48 copy (+ role-fix)
        // cost-cap BEFORE any provider call (S1-R55, D17).
        if let Err(e) = ctx.cost_guard.check_before_call() {
            return Ok(TurnResult::Refused(e));
        }
        // The finalize call advertises NO tools, so the model must answer in prose.
        let tools = if finalizing { Vec::new() } else { ctx.tools.defs() };
        let req = crate::dispatch::DispatchRequest {
            messages: api_msgs,
            tools,
            params: ctx.params.clone(),
            tenant: ctx.tenant,
        };
        let nr = harness
            .dispatch
            .complete(&req, cancel.clone())
            .await
            .map_err(|e| e.to_string())?;
        if let Some(u) = nr.usage {
            total_usage.input += u.input;
            total_usage.output += u.output;
            total_usage.cache_read += u.cache_read;
            total_usage.cache_write += u.cache_write;
            total_usage.reasoning += u.reasoning;
            ctx.budget.consume_tokens(turn_tokens(&u)); // BUD-R1
        }
        ctx.budget.tick_round(); // BUD-R2
        let assistant = build_assistant_message(&nr);
        messages.push(assistant.clone());
        // On the finalizing call we asked for no tools — return whatever answer we got as the turn result.
        if finalizing {
            if let Some(sink) = ctx.sink.as_ref() {
                sink.on_assistant_delta(ctx.budget.round(), nr.content.as_deref().unwrap_or(""));
            }
            ctx.persistence.persist_assistant(&assistant); // epilogue (S1-R56)
            return Ok(TurnResult::Final {
                content: nr.content,
                usage: total_usage,
                finish_reason: nr.finish_reason,
            });
        }
        match &nr.tool_calls {
            Some(tcs) if !tcs.is_empty() => {
                if let Some(sink) = ctx.sink.as_ref() {
                    for tc in tcs {
                        sink.on_tool_call(ctx.budget.round(), &tc.name, &tc.arguments);
                    }
                }
                let results = ctx.tools.dispatch(tcs).await;
                if let Some(sink) = ctx.sink.as_ref() {
                    for r in &results {
                        sink.on_tool_result(
                            ctx.budget.round(),
                            r.tool_name.as_deref().unwrap_or(""),
                            r.content.as_deref().unwrap_or(""),
                        );
                    }
                }
                messages.extend(results);
                // a stub scripted to loop forever keeps emitting tool calls → the round backstop
                // flips `finalizing` true on a later pass, which forces a tool-less answer.
            }
            _ => {
                if let Some(sink) = ctx.sink.as_ref() {
                    sink.on_assistant_delta(ctx.budget.round(), nr.content.as_deref().unwrap_or(""));
                }
                ctx.persistence.persist_assistant(&assistant); // epilogue (S1-R56)
                return Ok(TurnResult::Final {
                    content: nr.content,
                    usage: total_usage,
                    finish_reason: nr.finish_reason,
                });
            }
        }
    }
}

#[cfg(test)]
mod budget_tests {
    use super::Budget;

    #[test]
    fn exhausts_on_tokens_or_rounds() {
        // token ceiling
        let mut b = Budget::tokens(100, 8);
        assert!(!b.exhausted());
        b.consume_tokens(60);
        assert!(!b.exhausted());
        b.consume_tokens(60); // saturating → 0
        assert!(b.exhausted(), "spent token ceiling → exhausted");

        // round backstop (tokens never run out, but rounds do)
        let mut r = Budget::tokens(1_000_000, 2);
        r.tick_round();
        assert!(!r.exhausted());
        r.tick_round();
        assert!(r.exhausted(), "hit round backstop → exhausted");
        assert_eq!(r.round(), 2);
    }
}
