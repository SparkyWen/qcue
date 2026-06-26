//! QCue S3 — the recall / wiki-query SSE driver. Runs an AGENTIC recall turn (Appendix A: recall is NOT a
//! closed-book retrieval) through `AppState::recall_llm` — in production `RouterWikiLlm::live_recall`,
//! which advertises AND really executes `recall_search`/`read_page`/`read_lines` so the MODEL drives its
//! own search over the tenant's captures/wiki. It emits the Appendix A §3.4 recall taxonomy as
//! `RuntimeEventEnvelope`s onto the per-Thread `StreamHub`:
//!
//!   `session_started → (tool_call → tool_result)* → message_delta → citation* → usage → done`
//!
//! (or a terminal `error`). The `(tool_call → tool_result)*` pairs are the model's REAL agentic searches,
//! streamed mid-turn by `StreamHubSink` (zero of them when the model answers from its own knowledge). The
//! system prompt (`ideas::recall::prompt::build_recall_prompt`) keeps the
//! model's FULL general knowledge and frames the wiki as a tool, NOT a cage — this is the fix for the old
//! `build_synthesis_prompt` path whose rule was literally "answer from the wiki, not general knowledge",
//! which made the assistant unable to answer anything outside the user's notes. The synthesized answer's
//! `## References` block becomes the `citation` events. The driver runs on its own task (the route spawns
//! it) so a slow SSE consumer back-pressures only its own broadcast buffer (S3-R40) and the replay ring
//! backfills a reconnect (S3-R37/R38).
use crate::state::AppState;
use crate::wire::hub::StreamHub;
use app_server_protocol::RuntimeEventEnvelope;
use fence::fence_untrusted;
use ideas::recall::prompt::build_recall_prompt;
use protocol::{CanonicalUsage, Citation, Message, Role, TurnEventSink};
use sqlx::PgPool;
use std::sync::{Arc, OnceLock};
use store::messages_repo::{ConversationsRepo, MessagesRepo};
use tokio::sync::{Semaphore, TryAcquireError};
use store::wiki_repo::WikiRepo;
use uuid::Uuid;
use wiki::index_gen::regenerate_index;
use wiki::llm::{RecallOverride, SystemBlocks, WikiReq};
use wiki::query::parse_references;

/// Recall answers are bounded; the model may take several tool iterations inside the turn loop first.
const RECALL_MAX_TOKENS: u32 = 2048;

/// The streamed `tool_result` content is capped — the 256-slot broadcast + 20-event replay ring should
/// carry a readable head, not an 8KB page body (the MODEL still gets the full result via the Tool message).
const TOOL_RESULT_PREVIEW_CHARS: usize = 600;

/// A [`TurnEventSink`] that publishes the turn's REAL tool steps onto the per-Thread `StreamHub` as the
/// §3.4 recall taxonomy, so each model-authored `recall_search`/`read_page` becomes its own
/// `tool_call`+`tool_result` SSE event (per-tool-call streaming). Called INLINE on the turn task; the
/// hub's broadcast send never awaits, so a slow SSE consumer back-pressures only its own buffer (S3-R40).
/// The final answer is NOT streamed here — recall is non-streaming, so the whole answer arrives once as
/// the terminal `message_delta` (see [`emit_answer_taxonomy`]); streaming it again would double it.
struct StreamHubSink {
    hub: StreamHub,
    thread: Uuid,
}

impl StreamHubSink {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        let seq = self.hub.next_seq(self.thread);
        self.hub.publish(RuntimeEventEnvelope {
            schema_version: 1,
            thread_id: self.thread,
            turn_id: None,
            seq,
            event: event.to_string(),
            payload,
        });
    }
}

impl TurnEventSink for StreamHubSink {
    fn on_tool_call(&self, _iter: u32, name: &str, arguments: &str) {
        // Keep the model's verbatim args (A-R13); render as JSON when parseable so the client can read them.
        let args: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::Value::String(arguments.to_string()));
        self.emit("tool_call", serde_json::json!({ "tool": name, "args": args }));
    }
    fn on_tool_result(&self, _iter: u32, name: &str, content: &str) {
        let preview: String = content.chars().take(TOOL_RESULT_PREVIEW_CHARS).collect();
        self.emit("tool_result", serde_json::json!({ "tool": name, "result": preview }));
    }
    // on_assistant_delta: intentionally the default no-op (the recall path keeps the single terminal
    // message_delta with the whole answer; create_message is non-streaming).
}

/// Which surface drives the stream — both share the engine + taxonomy; the only difference is the
/// `session_started` payload label so the client can title the screen.
#[derive(Clone, Copy, Debug)]
pub enum RecallMode {
    Recall,
    WikiQuery,
}

impl RecallMode {
    fn label(self) -> &'static str {
        match self {
            RecallMode::Recall => "recall",
            RecallMode::WikiQuery => "wiki_query",
        }
    }
}

/// Global cap on concurrently-running agentic recall/wiki-query turns (both the SSE driver and the WSS
/// engine acquire from it). Each turn is heavy (multi-round tool loop + provider calls + short DB txs),
/// so without a cap a burst of GETs — each a fresh thread UUID, so the per-thread debounce doesn't apply
/// — could saturate CPU and starve the 16-connection DB pool. Over the cap → a transient OVERLOADED
/// refusal (the client retries), independent of the per-IP rate limit and the per-tenant cost cap.
const MAX_CONCURRENT_RECALL_TURNS: usize = 32;
pub(crate) fn recall_concurrency() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(MAX_CONCURRENT_RECALL_TURNS))
}

/// Build the OPEN agentic recall request (cache-safe system prefix + the fenced untrusted question) for
/// `tenant`. Shared by the SSE recall driver AND the WSS turn engine so both speak the same open,
/// tool-augmented prompt (full general knowledge + the wiki as tools, never closed-book). The wiki index
/// rides along as a table-of-contents hint — best-effort, so recall never breaks if it can't be built.
pub async fn build_recall_request(
    pool: &PgPool,
    tenant: Uuid,
    thread: Uuid,
    question: &str,
    prefer_wiki: bool,
    provider_display: &str,
    model: &str,
) -> WikiReq {
    let repo = WikiRepo::new(pool.clone());
    let index = regenerate_index(tenant, &repo).await.unwrap_or_default();

    // REC-R5/REC-D5: on continue, replay prior turns into the message TAIL (untrusted), ordered by seq,
    // BEFORE the fenced current question. The system prefix is NOT touched (byte-stable; prompt cache).
    // History load is best-effort: a read failure degrades to a fresh single-turn request.
    let history = MessagesRepo::new(pool.clone())
        .read_session(tenant, thread)
        .await
        .unwrap_or_default();
    let mut messages: Vec<Message> = history
        .into_iter()
        .filter_map(|m| {
            let role = match m.role.as_str() {
                "assistant" => Role::Assistant,
                "user" => Role::User,
                _ => return None, // only user/assistant turns are replayed (no system/tool steps)
            };
            let content = m.content?;
            // prior turns are untrusted, fenced like any tail content (S1-R38 / pitfall #1).
            let body = if role == Role::User { fence_untrusted("prior_user_query", &content) } else { content };
            Some(history_message(role, body))
        })
        .collect();

    // The current untrusted question lives in the fenced tail AFTER the replayed history.
    messages.push(Message {
        role: Role::User,
        content: Some(fence_untrusted("user_query", question)),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        finish_reason: None,
        reasoning: None,
        provider_data: None,
        active: true,
        is_untrusted: true,
    });

    // Advertise the web tools in the prompt IFF the harness actually wires them (kill-switch aware), so
    // the model is never told about a tool it cannot call.
    let allow_web = crate::dispatch::web_tools_enabled();
    WikiReq {
        system: SystemBlocks {
            stable_prefix: build_recall_prompt(&index, prefer_wiki, provider_display, model, allow_web),
        },
        messages,
        response_format: None,
        max_tokens: RECALL_MAX_TOKENS,
        cache_breakpoint: Some(1),
        disable_thinking: false,
    }
}

/// Build an untrusted tail message for a replayed prior turn (REC-D5). The turn loop's
/// `repair_role_alternation` later coalesces any same-role runs, so this never breaks alternation.
fn history_message(role: Role, content: String) -> Message {
    Message {
        role,
        content: Some(content),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        finish_reason: None,
        reasoning: None,
        provider_data: None,
        active: true,
        is_untrusted: true,
    }
}

/// Resolve the (provider, model) the harness will route this tenant to, for IDENTITY reporting in the
/// system prompt. Mirrors `build_harness_for`: the keyless stub reports `("stub","stub")`; otherwise the
/// tenant's effective BYOK route. The two resolve from the same source, so the prompt's stated identity
/// matches the provider that actually answers.
async fn resolve_identity(pool: &PgPool, tenant: Uuid) -> (String, String) {
    if crate::dispatch::stub_llm_enabled() {
        return ("stub".to_string(), "stub".to_string());
    }
    crate::dispatch::effective_route(pool, tenant).await
}

/// Drive an AGENTIC recall/wiki-query turn for `question` and publish the §3.4 SSE taxonomy onto
/// `st.threads` keyed by `thread`. Always terminates with exactly one `done` (on success) or `error`.
///
/// The model gets an OPEN, tool-augmented system prompt (full general knowledge + the wiki as tools) and
/// the untrusted question fenced in the message tail (S1-R38 / pitfall #1). `recall_llm` runs the real
/// `router::run_turn` loop: when the question is about the user's notes the model authors `recall_search`
/// and the result is fed back before it answers; otherwise it answers directly from its own knowledge.
pub async fn run_recall_stream(
    st: &AppState,
    tenant: Uuid,
    user: Uuid,
    thread: Uuid,
    question: &str,
    mode: RecallMode,
    over: RecallOverride,
) {
    // session_started — opens the recall session (A-R34).
    publish(st, tenant, thread, "session_started", serde_json::json!({ "mode": mode.label() }));

    // D17/B-R20 — enforce the daily cost ceiling BEFORE the (most expensive, multi-round, web-tool)
    // recall provider call. The in-loop wiki cost guard is a no-op (AllowCost), so this controller-layer
    // check is the only thing stopping a tenant over their cap from issuing unlimited recall turns.
    match store::cost_repo::CostRepo::new(st.pool.clone()).check_ceiling(tenant, user).await {
        Ok(Ok(())) => {} // under both ceilings — proceed.
        Ok(Err(reason)) => {
            publish(
                st,
                tenant,
                thread,
                "error",
                serde_json::json!({ "code": app_server_protocol::error_codes::COST_CEILING, "message": reason }),
            );
            st.threads.close(thread);
            return;
        }
        Err(e) => {
            tracing::warn!(error = %e, %tenant, "recall cost-ceiling check failed");
            publish(
                st,
                tenant,
                thread,
                "error",
                serde_json::json!({ "code": app_server_protocol::error_codes::OVERLOADED, "message": "cost check failed; try again" }),
            );
            st.threads.close(thread);
            return;
        }
    }

    // Bound concurrent heavy turns (DoS): hold a permit for the whole turn. Over the cap → transient
    // OVERLOADED so the client retries, rather than piling unbounded agentic turns onto the runtime.
    let _permit = match recall_concurrency().try_acquire() {
        Ok(p) => p,
        Err(TryAcquireError::NoPermits) => {
            publish(
                st,
                tenant,
                thread,
                "error",
                serde_json::json!({ "code": app_server_protocol::error_codes::OVERLOADED, "message": "server busy; please retry shortly" }),
            );
            st.threads.close(thread);
            return;
        }
        Err(TryAcquireError::Closed) => return,
    };

    // Resolve the (provider, model) the system prompt states as the model's identity. v0.2.2: an explicit
    // recall picker override wins (so the prompt names the model the user chose), EXCEPT under the keyless
    // stub, which always reports a neutral identity (never a real vendor). Otherwise this mirrors the
    // tenant's effective route, so the prompt and the actual call agree.
    let (provider, model) = match (!crate::dispatch::stub_llm_enabled())
        .then(|| over.route())
        .flatten()
    {
        Some(route) => route,
        None => resolve_identity(&st.pool, tenant).await,
    };
    let provider_display = crate::dispatch::provider_display_name(&provider).to_string();

    // Build the request FIRST (it loads prior history via read_session) so the just-asked question is
    // NOT yet in `messages` — it appears exactly once (the fenced current question), matching the WSS
    // path and never duplicating on a fresh turn (REC-R5/S1-R38).
    let prefer_wiki = matches!(mode, RecallMode::WikiQuery);
    let req =
        build_recall_request(&st.pool, tenant, thread, question, prefer_wiki, &provider_display, &model).await;

    // REC-R1/REC-D6: persist the user question + upsert the conversation header AFTER building the
    // request but BEFORE the provider call (create_message_observed) — durable before the model runs
    // (S1-R56). Persistence is best-effort: a write hiccup must never abort the live answer.
    let msgs_repo = MessagesRepo::new(st.pool.clone());
    let convo_repo = ConversationsRepo::new(st.pool.clone());
    let _ = msgs_repo.insert_user(tenant, user, thread, question).await;
    let _ = convo_repo.upsert(tenant, user, thread, question).await;

    // The observer publishes each REAL tool_call/tool_result mid-turn (per-tool-call streaming); the
    // tool steps fire as a side effect DURING create_message_observed, in the model's own order.
    let sink: Arc<dyn TurnEventSink> = Arc::new(StreamHubSink { hub: st.threads.clone(), thread });
    // v0.2.2 — carry the per-recall route + effort override into the turn (empty = tenant default).
    match st
        .recall_llm
        .create_message_observed_with_override(tenant, req, Some(sink), over)
        .await
    {
        Ok(resp) => {
            // REC-R1/REC-D6: persist the FINAL assistant text only (no tool steps), keyed by thread.
            let _ = msgs_repo.insert_assistant(tenant, user, thread, &resp.content).await;
            let citations = parse_references(&resp.content);
            emit_answer_taxonomy(st, tenant, thread, &resp.content, &citations, resp.usage);
        }
        Err(e) => {
            let (code, message) = recall_error_event(&e.to_string());
            publish(
                st,
                tenant,
                thread,
                "error",
                serde_json::json!({ "code": code, "message": message }),
            );
        }
    }
    st.threads.close(thread);
}

/// Emit the success taxonomy: a `recall_search` tool_call + tool_result affordance, the answer
/// `message_delta`, the `citation`s parsed from `## References`, the real provider `usage`, then `done`.
///
/// NOTE: the model's REAL `tool_call`/`tool_result` events were already streamed mid-turn by
/// [`StreamHubSink`] (per-tool-call streaming), in the model's own order — there is NO synthetic
/// affordance here. This emits only the terminal answer + citations + usage + done.
fn emit_answer_taxonomy(
    st: &AppState,
    tenant: Uuid,
    thread: Uuid,
    answer: &str,
    citations: &[Citation],
    usage: Option<CanonicalUsage>,
) {
    // message_delta — the model's answer (one delta; recall is non-streaming, so the whole answer once).
    publish(st, tenant, thread, "message_delta", serde_json::json!({ "delta": answer }));
    // citation* — one per parsed `## References` entry (A-R25: first-class, line-safe).
    for c in citations {
        publish(
            st,
            tenant,
            thread,
            "citation",
            serde_json::json!({ "rel_path": c.rel_path, "start_line": c.start_line, "end_line": c.end_line }),
        );
    }
    // usage — the REAL provider tally for the whole turn (priced + accrued in `RouterWikiLlm`); zeros if
    // the (test) stub did not report any.
    let u = usage.unwrap_or_default();
    publish(
        st,
        tenant,
        thread,
        "usage",
        serde_json::json!({
            "input": u.input, "output": u.output, "cache_read": u.cache_read,
            "cache_write": u.cache_write, "reasoning": u.reasoning
        }),
    );
    // done — the terminal success marker.
    publish(st, tenant, thread, "done", serde_json::json!({ "ok": true }));
}

/// Map a recall failure to a user-facing `(code, message)` for the SSE `error` event. A missing/invalid
/// BYOK credential is a CONFIG error (`NO_CREDENTIALS`) with an actionable message — NOT a transient
/// overload (the client retries `-32001`) and NOT a raw internal transport string. Other failures keep
/// the overload code (existing retry behavior) but with a friendlier prefix instead of the raw error.
fn recall_error_event(err: &str) -> (i32, String) {
    use app_server_protocol::error_codes;
    let lc = err.to_ascii_lowercase();
    let is_credential_config = lc.contains("no credentials")
        || lc.contains("no eligible credential")
        || lc.contains("decrypt failed");
    if is_credential_config {
        (
            error_codes::NO_CREDENTIALS,
            "Recall needs an API key. Add a provider key (e.g. DeepSeek) in Settings to enable it."
                .to_string(),
        )
    } else {
        (error_codes::OVERLOADED, format!("Recall couldn't complete: {err}"))
    }
}

/// Build + publish one `RuntimeEventEnvelope` with a monotonic `seq` from the hub (so the replay ring
/// can backfill a reconnect). `event` is the forward-compat wire String (the §3.4 taxonomy token).
fn publish(st: &AppState, _tenant: Uuid, thread: Uuid, event: &str, payload: serde_json::Value) {
    let seq = st.threads.next_seq(thread);
    let env = RuntimeEventEnvelope {
        schema_version: 1,
        thread_id: thread,
        turn_id: None,
        seq,
        event: event.to_string(),
        payload,
    };
    st.threads.publish(env);
}

#[cfg(test)]
mod tests {
    use super::recall_error_event;
    use app_server_protocol::error_codes;

    #[test]
    fn a_missing_credential_is_a_friendly_config_error_not_overload() {
        // The exact dispatch string for a tenant whose configured provider has no usable key.
        let (code, msg) = recall_error_event("provider error: transport: no credentials for deepseek");
        assert_eq!(code, error_codes::NO_CREDENTIALS, "must NOT be the retryable OVERLOADED code");
        assert_ne!(code, error_codes::OVERLOADED);
        assert_ne!(code, error_codes::UNAUTHORIZED, "must NOT trigger the JWT refresh/login flow");
        assert!(msg.to_lowercase().contains("api key"), "actionable message: {msg}");
        assert!(!msg.contains("transport"), "no raw internal error leaks to the user: {msg}");
    }

    #[test]
    fn a_no_eligible_credential_is_also_a_config_error() {
        let (code, _) = recall_error_event("no eligible credential for deepseek");
        assert_eq!(code, error_codes::NO_CREDENTIALS);
    }

    #[test]
    fn other_failures_stay_overload_but_are_humanized() {
        let (code, msg) = recall_error_event("upstream 500 boom");
        assert_eq!(code, error_codes::OVERLOADED);
        assert!(msg.starts_with("Recall couldn't complete"), "humanized prefix: {msg}");
    }
}
