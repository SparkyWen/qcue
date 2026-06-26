#![allow(clippy::unwrap_used, clippy::expect_used)]
//! LIVE, network-hitting verification that the REAL harness drives provider-native tool calling
//! end-to-end on every first-class vendor. It exercises the WHOLE stack the user actually hits:
//! `router::run_turn` (the one agentic loop) → `HttpDispatch` (the only live call site) → the REAL
//! registered provider profiles (base_url / headers / hooks / api_mode) → `transport_for(api_mode)`
//! (the ChatCompletions + Anthropic wires) → tool dispatch → loop continuation.
//!
//! It advertises the ACTUAL recall tool policy (`recall_search`/`read_page`/`read_lines`) and the ACTUAL
//! recall system prompt, then verifies TWO things per vendor:
//!
//!   * `*_calls_recall_tool_and_grounds_answer` — a SINGLE tool call: the model authors a recall tool,
//!     the loop feeds the result back (the 2nd provider call — the bit that 400'd on Anthropic), and the
//!     final answer is grounded in a codeword only the tool returned.
//!   * `*_chains_multiple_tools` — a MULTI-tool, MULTI-iteration chain: the model must call `recall_search`
//!     (which returns a POINTER to a page, not the answer) and THEN `read_page` to read the codeword, then
//!     answer. Proves the full agentic tool loop (several iterations, more than one distinct tool) works.
//!
//! These tests are `#[ignore]` (they cost real tokens and need real keys). Run them with the keys in env:
//!   OPENAI_API_KEY=sk-... DEEPSEEK_API_KEY=sk-... ANTHROPIC_API_KEY=sk-ant-... \
//!     cargo test -p app-server --test live_tool_calling -- --ignored --nocapture
//!
//! A missing key fails its test with a clear message (so a partial key set still verifies what it can).

use async_trait::async_trait;
use ideas::recall::prompt::build_recall_prompt;
use ideas::recall::tool_policy::build_tool_policy;
use protocol::{ApiMode, CredStatus, Message, Role};
use router::dispatch_http::HttpDispatch;
use router::pool::{CredentialPool, PoolStrategy, PooledCredential};
use router::resolver::{CredentialResolver, ResolveError};
use router::retry_loop::FallbackChain;
use router::tools::{ToolDispatcher, ToolExec};
use router::transport::ReqParams;
use router::turn::{run_turn, Budget, CostGuard, Harness, Persistence, TurnContext, TurnResult};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const AAD: &str = "live-tenant";
/// A codeword that exists ONLY in a tool result — the final answer can only contain it if the loop fed
/// the tool result back and the model read it.
const CODEWORD: &str = "ZIRCON-7";

/// A resolver that hands out the env-provided API key, sealed/unsealed through the REAL ZeroizingKey path
/// (so the live test exercises the same decrypt seam production uses).
struct LiveResolver {
    key: String,
}
#[async_trait]
impl CredentialResolver for LiveResolver {
    async fn pool_for(&self, _t: Uuid, _p: &str) -> Result<CredentialPool, ResolveError> {
        let cred = PooledCredential {
            id: Uuid::nil(),
            label: None,
            priority: 0,
            status: CredStatus::Ok,
            key_hint: "live".into(),
            last_error_code: None,
            last_error_reason: None,
            request_count: 0,
        };
        Ok(CredentialPool::new(vec![cred], PoolStrategy::FillFirst))
    }
    async fn decrypt(&self, _t: Uuid, _c: Uuid) -> Result<secrets::ZeroizingKey, ResolveError> {
        let kms = secrets::StubKms::new();
        let enc = secrets::EncryptedCredential::seal(&kms, self.key.as_bytes(), AAD)
            .map_err(|e| ResolveError::Decrypt(e.to_string()))?;
        secrets::decrypt_with_tenant(&kms, &enc, AAD).map_err(|e| ResolveError::Decrypt(e.to_string()))
    }
}

/// SINGLE-tool handler: returns a canned, citable hit that carries the codeword for ANY recall tool.
struct SpyRecall {
    calls: Arc<Mutex<Vec<(String, String)>>>,
}
#[async_trait]
impl ToolExec for SpyRecall {
    async fn call(&self, name: &str, args: &str) -> Result<String, String> {
        self.calls.lock().unwrap().push((name.into(), args.into()));
        Ok(format!(
            "Found 1 result:\n[1] postgres-migrations.md:1\n  goal: postgres migration strategy\n  \
             conclusion: the team settled on EXPAND-MIGRATE-CONTRACT with a 7-day dual-write window; \
             the agreed codeword for this decision is {CODEWORD}.\n  window: We concluded the safest \
             path is expand/contract with a 7-day dual-write window. Codeword {CODEWORD}."
        ))
    }
}

/// MULTI-tool handler: `recall_search` returns a POINTER to a page WITHOUT the codeword, so the model MUST
/// chain a `read_page` call to actually read the codeword. This forces a multi-iteration, multi-tool turn.
struct SpyMultiTool {
    calls: Arc<Mutex<Vec<(String, String)>>>,
}
#[async_trait]
impl ToolExec for SpyMultiTool {
    async fn call(&self, name: &str, args: &str) -> Result<String, String> {
        self.calls.lock().unwrap().push((name.into(), args.into()));
        match name {
            "recall_search" => Ok(
                "Found 1 result:\n[1] postgres-migrations.md — goal: the postgres migration decision.\n\
                 NOTE: the decision codeword is NOT in this snippet — it is recorded in the page body. \
                 Call read_page(slug=\"postgres-migrations\") to read it."
                    .into(),
            ),
            "read_page" => {
                // Only yield the codeword when the model reads the RIGHT page (proves it used the pointer).
                if args.contains("postgres-migrations") {
                    Ok(format!(
                        "# Postgres Migrations\n\nThe team concluded: expand/migrate/contract with a \
                         7-day dual-write window. The agreed decision codeword is {CODEWORD}."
                    ))
                } else {
                    Ok("page not found: pass slug=\"postgres-migrations\"".into())
                }
            }
            "read_lines" => Ok("(use read_page for the whole page)".into()),
            other => Err(format!("unexpected tool {other}")),
        }
    }
}

struct NoPersist;
impl Persistence for NoPersist {
    fn persist_user(&self, _m: &Message) {}
    fn persist_assistant(&self, _m: &Message) {}
}
struct AllowCost;
impl CostGuard for AllowCost {
    fn check_before_call(&self) -> Result<(), String> {
        Ok(())
    }
}

fn sys_msg(content: String) -> Message {
    msg(Role::System, content, false)
}
fn user_msg(content: &str) -> Message {
    msg(Role::User, content.to_string(), true)
}
fn msg(role: Role, content: String, untrusted: bool) -> Message {
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
        is_untrusted: untrusted,
    }
}

/// Drive one full agentic recall turn against the LIVE provider through the real harness, with a
/// caller-supplied tool handler + question; return the turn result and the tool calls the model authored.
async fn run_live_recall(
    provider: &str,
    model: &str,
    key: String,
    exec: Arc<dyn ToolExec>,
    question: &str,
    effort: Option<providers::hooks::Effort>,
) -> (TurnResult, Vec<(String, String)>) {
    let registry = Arc::new(providers::registry::register_all());
    let api_mode =
        registry.get(provider).map(|p| p.api_mode).unwrap_or(ApiMode::ChatCompletions);
    let resolver = Arc::new(LiveResolver { key });
    let chain = FallbackChain::new(vec![(provider.into(), model.into(), api_mode)]);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .expect("reqwest client");
    // allow_insecure=false: the real vendor endpoints are all https.
    let dispatch = HttpDispatch::new(client, registry, resolver, chain, false);
    let harness = Harness::with_dispatch(Box::new(dispatch));

    // The ACTUAL recall tool policy the production path advertises (read tools + live-internet tools).
    let tools = ToolDispatcher::with_handler(build_tool_policy(false, true).tools, exec);

    let history = vec![
        sys_msg(build_recall_prompt(
            "# Index\n- [[postgres-migrations|Postgres Migrations]] — the migration decision\n",
            false,
            app_server::dispatch::provider_display_name(provider),
            model,
            true,
        )),
        user_msg(question),
    ];
    let ctx = TurnContext {
        history,
        budget: Budget::tokens(250_000, 32),
        // A generous output budget so a reasoning model (gpt-5.x) has room for the final answer after it
        // spends tokens thinking. `effort` (when set) exercises the Responses path: gpt-5.x carries a native
        // reasoning:{effort} object alongside the function tools — the combo that 400'd on chat/completions.
        params: ReqParams {
            max_tokens: Some(4096),
            reasoning: effort.map(|e| providers::hooks::ReasoningConfig { effort: Some(e) }),
            ..Default::default()
        },
        persistence: Box::new(NoPersist),
        cost_guard: Box::new(AllowCost),
        tools,
        tenant: Uuid::nil(),
        sink: None,
    };
    let res = run_turn(&harness, ctx, CancellationToken::new()).await.expect("turn runs to completion");
    // calls are recorded on the exec; pull them out via the dedicated accessor below.
    (res, Vec::new())
}

fn key_or_panic(var: &str) -> String {
    std::env::var(var)
        .ok()
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| panic!("set {var} to run this live test (it makes a real API call)"))
}

const RECALL_TOOLS: &[&str] = &["recall_search", "read_page", "read_lines"];

/// SINGLE-tool assertion: a recall tool was called, the loop completed, and the answer is grounded.
fn assert_called_and_grounded(provider: &str, res: TurnResult, seen: &[(String, String)]) {
    assert!(
        seen.iter().any(|(n, _)| RECALL_TOOLS.contains(&n.as_str())),
        "[{provider}] the model must author a recall tool through the loop; saw: {seen:?}",
    );
    let answer = expect_final(provider, res);
    assert!(
        answer.to_uppercase().contains(CODEWORD),
        "[{provider}] the final answer must be GROUNDED in the tool result (contain {CODEWORD:?}); got: {answer:?}",
    );
    eprintln!("[{provider}] single-tool OK — grounded answer: {answer}");
}

/// MULTI-tool assertion: BOTH recall_search and read_page were called (a chain of ≥2 distinct tools across
/// ≥2 loop iterations), and the final answer is grounded in the page the model had to read.
fn assert_chained_and_grounded(provider: &str, res: TurnResult, seen: &[(String, String)]) {
    let names: Vec<&str> = seen.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"recall_search"), "[{provider}] must call recall_search first; saw: {seen:?}");
    assert!(
        names.contains(&"read_page"),
        "[{provider}] must CHAIN a read_page after the search pointed at the page; saw: {seen:?}",
    );
    assert!(seen.len() >= 2, "[{provider}] a multi-tool chain runs ≥2 tool iterations; saw: {seen:?}");
    let answer = expect_final(provider, res);
    assert!(
        answer.to_uppercase().contains(CODEWORD),
        "[{provider}] the answer must be grounded in the page the model READ (contain {CODEWORD:?}); got: {answer:?}",
    );
    eprintln!("[{provider}] multi-tool OK — chain={names:?} grounded answer: {answer}");
}

fn expect_final(provider: &str, res: TurnResult) -> String {
    match res {
        TurnResult::Final { content, finish_reason, .. } => {
            assert_ne!(
                finish_reason,
                protocol::FinishReason::Length,
                "[{provider}] the answer should complete, not truncate",
            );
            content.unwrap_or_default()
        }
        other => panic!("[{provider}] expected a Final answer, got {other:?}"),
    }
}

const SINGLE_Q: &str =
    "What did I conclude about postgres migrations in my own notes? Use the recall_search tool to look it \
     up, then tell me the exact codeword for that decision.";
const MULTI_Q: &str =
    "Find the postgres migration decision in my own notes and tell me its exact codeword. Search for it \
     first; the search result will point you to a specific wiki page — then READ that page to get the \
     codeword. Do not guess.";

async fn single(provider: &str, model: &str, key: String) -> (TurnResult, Vec<(String, String)>) {
    single_effort(provider, model, key, None).await
}
async fn multi(provider: &str, model: &str, key: String) -> (TurnResult, Vec<(String, String)>) {
    multi_effort(provider, model, key, None).await
}
async fn single_effort(
    provider: &str,
    model: &str,
    key: String,
    effort: Option<providers::hooks::Effort>,
) -> (TurnResult, Vec<(String, String)>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (res, _) =
        run_live_recall(provider, model, key, Arc::new(SpyRecall { calls: calls.clone() }), SINGLE_Q, effort).await;
    let seen = calls.lock().unwrap().clone();
    (res, seen)
}
async fn multi_effort(
    provider: &str,
    model: &str,
    key: String,
    effort: Option<providers::hooks::Effort>,
) -> (TurnResult, Vec<(String, String)>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (res, _) =
        run_live_recall(provider, model, key, Arc::new(SpyMultiTool { calls: calls.clone() }), MULTI_Q, effort).await;
    let seen = calls.lock().unwrap().clone();
    (res, seen)
}

// ─────────────────────────── OpenAI (gpt-5.5, the newest) ───────────────────────────
#[tokio::test]
#[ignore = "live: makes a real OpenAI API call; needs OPENAI_API_KEY"]
async fn openai_gpt5_5_calls_recall_tool_and_grounds_answer() {
    let key = key_or_panic("OPENAI_API_KEY");
    let (res, seen) = single("openai", "gpt-5.5", key).await;
    assert_called_and_grounded("openai/gpt-5.5", res, &seen);
}
#[tokio::test]
#[ignore = "live: makes a real OpenAI API call; needs OPENAI_API_KEY"]
async fn openai_gpt5_5_chains_multiple_tools() {
    let key = key_or_panic("OPENAI_API_KEY");
    let (res, seen) = multi("openai", "gpt-5.5", key).await;
    assert_chained_and_grounded("openai/gpt-5.5", res, &seen);
}
// D19 / RESP-* — the BLIND SPOT that the chat-completions stop-gap left open: gpt-5.5 with a reasoning
// EFFORT set AND function tools. On chat/completions this 400'd ("Function tools with reasoning_effort are
// not supported for gpt-5.5 in /v1/chat/completions"); these drive the model-aware route to /v1/responses,
// where reasoning effort + tools coexist. The whole point of the Responses transport — verify it LIVE.
#[tokio::test]
#[ignore = "live: makes a real OpenAI /v1/responses call; needs OPENAI_API_KEY"]
async fn openai_gpt5_5_with_effort_calls_recall_tool_and_grounds_answer() {
    let key = key_or_panic("OPENAI_API_KEY");
    let (res, seen) = single_effort("openai", "gpt-5.5", key, Some(providers::hooks::Effort::High)).await;
    assert_called_and_grounded("openai/gpt-5.5+effort", res, &seen);
}
#[tokio::test]
#[ignore = "live: makes a real OpenAI /v1/responses call; needs OPENAI_API_KEY"]
async fn openai_gpt5_5_with_effort_chains_multiple_tools() {
    let key = key_or_panic("OPENAI_API_KEY");
    let (res, seen) = multi_effort("openai", "gpt-5.5", key, Some(providers::hooks::Effort::High)).await;
    assert_chained_and_grounded("openai/gpt-5.5+effort", res, &seen);
}

// ─────────────────────────── DeepSeek (deepseek-v4-pro) ───────────────────────────
#[tokio::test]
#[ignore = "live: makes a real DeepSeek API call; needs DEEPSEEK_API_KEY"]
async fn deepseek_v4_pro_calls_recall_tool_and_grounds_answer() {
    let key = key_or_panic("DEEPSEEK_API_KEY");
    let (res, seen) = single("deepseek", "deepseek-v4-pro", key).await;
    assert_called_and_grounded("deepseek/deepseek-v4-pro", res, &seen);
}
#[tokio::test]
#[ignore = "live: makes a real DeepSeek API call; needs DEEPSEEK_API_KEY"]
async fn deepseek_v4_pro_chains_multiple_tools() {
    let key = key_or_panic("DEEPSEEK_API_KEY");
    let (res, seen) = multi("deepseek", "deepseek-v4-pro", key).await;
    assert_chained_and_grounded("deepseek/deepseek-v4-pro", res, &seen);
}

// ─────────────────────────── Anthropic (claude-opus-4-8) ───────────────────────────
#[tokio::test]
#[ignore = "live: makes a real Anthropic API call; needs ANTHROPIC_API_KEY"]
async fn anthropic_opus_4_8_calls_recall_tool_and_grounds_answer() {
    // This is the case that 400'd before the tool_result fix: the model calls a recall tool on the first
    // iteration, then the loop's SECOND request must replay the result as a tool_result block.
    let key = key_or_panic("ANTHROPIC_API_KEY");
    let (res, seen) = single("anthropic", "claude-opus-4-8", key).await;
    assert_called_and_grounded("anthropic/claude-opus-4-8", res, &seen);
}
#[tokio::test]
#[ignore = "live: makes a real Anthropic API call; needs ANTHROPIC_API_KEY"]
async fn anthropic_opus_4_8_chains_multiple_tools() {
    // The strongest proof the fix holds: multiple tool_result round-trips in ONE turn on Anthropic.
    let key = key_or_panic("ANTHROPIC_API_KEY");
    let (res, seen) = multi("anthropic", "claude-opus-4-8", key).await;
    assert_chained_and_grounded("anthropic/claude-opus-4-8", res, &seen);
}
