// QCue S2-R1 — RouterWikiLlm: the real `WikiLlm` impl that drives the S1 router's ONE turn loop
// (`router::run_turn`). It is the only place the wiki/ideas engine reaches a model; the wiki crate
// itself stays provider-agnostic (it sees only the `WikiLlm` trait). At this milestone the router's
// `Harness` is stub-backed (M1), so RouterWikiLlm is testable end-to-end without a network or keys —
// the same seam later swaps the harness's `call` for real transport with no change here.
use async_trait::async_trait;
use protocol::{Message, Role, ToolDef, TurnEventSink};
use router::stub::StubProvider;
use router::tools::ToolDispatcher;
use router::turn::{run_turn, Budget, CostGuard as RouterCostGuard, Harness, Persistence, TurnContext, TurnResult};
use router::tools::ToolExec;
use secrets::Kms;
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use wiki::llm::{RecallOverride, TenantId, WikiLlm, WikiLlmError, WikiReq, WikiResp};

/// No-op persistence — wiki extraction turns are stateless single calls (the idea + JSONL are already
/// persisted at capture; the wiki page bodies persist through the write-gate, not the turn loop).
struct NoPersist;
impl Persistence for NoPersist {
    fn persist_user(&self, _m: &Message) {}
    fn persist_assistant(&self, _m: &Message) {}
}

/// The cost ceiling is enforced by the wiki `CostGuard` BEFORE each WikiLlm call (S2-R19); inside the
/// router turn we allow (double-charging the per-call cap would be wrong).
struct AllowCost;
impl RouterCostGuard for AllowCost {
    fn check_before_call(&self) -> Result<(), String> {
        Ok(())
    }
}

/// What the recall path needs to build a tenant-bound `RecallToolExec` per turn: the pool (for the
/// `SearchRepo`) + the vault-root base (`<data_root>/objects`, joined with `t/<tenant>/u/_`) + the shared
/// live-internet `WebClient` (cheap to clone — reqwest is internally `Arc`; `None` keeps recall offline).
#[derive(Clone)]
struct RecallToolDeps {
    pool: PgPool,
    vault_root_base: PathBuf,
    web: Option<Arc<crate::web_tool::WebClient>>,
}

/// Build the shared web client + the recall tool set, honoring the web kill-switch
/// (`crate::dispatch::web_tools_enabled`). When web is enabled the policy advertises `web_search`/
/// `web_fetch` AND a live `WebClient` executes them; when disabled the tools are not advertised and no
/// client is wired (so the recall PROMPT and the recall TOOL SET always agree).
/// BUD-R1/R2 — the per-turn spend budget from env, with deep-recall-friendly defaults. The token
/// ceiling is the real cost proxy (the transcript re-sends each round); the round backstop bounds a
/// degenerate loop. Both are env-tunable in prod without a code change.
fn turn_budget_from_env() -> Budget {
    let tokens = std::env::var("QCUE_TURN_TOKEN_BUDGET")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(250_000);
    let rounds = std::env::var("QCUE_TURN_MAX_ROUNDS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(32);
    Budget::tokens(tokens, rounds)
}

fn recall_tools_and_web() -> (Vec<ToolDef>, Option<Arc<crate::web_tool::WebClient>>) {
    let allow_web = crate::dispatch::web_tools_enabled();
    let tools = ideas::recall::tool_policy::build_tool_policy(false, allow_web).tools;
    let web = allow_web.then(|| Arc::new(crate::web_tool::WebClient::new()));
    (tools, web)
}

/// Where each turn's `Harness` comes from. `Prebuilt` is a fixed harness (the keyless stub, or a
/// caller-supplied one in tests). `PerTenant` rebuilds the harness PER CALL against the turn's tenant,
/// so the `FallbackChain` is rooted at the provider/model THAT tenant configured (BYOK) — the fix for
/// the shared, boot-time, tenant-blind harness that always tried openai.
enum HarnessSource {
    Prebuilt(Harness),
    PerTenant { pool: PgPool, kms: Arc<dyn Kms + Send + Sync>, stub_reply: String },
}

pub struct RouterWikiLlm {
    source: HarnessSource,
    /// The tools the harness advertises to the model on each call. Empty for plain extraction; the
    /// recall/Dream paths advertise `recall_search`/`read_*`/`propose_*` so the model authors them.
    tools: Vec<ToolDef>,
    /// When present, the advertised tools EXECUTE for real (recall): the dispatcher routes the model's
    /// tool calls to a tenant-bound `RecallToolExec`. When absent, the tools are stub-executed.
    recall_deps: Option<RecallToolDeps>,
}

impl RouterWikiLlm {
    /// Construct from a fixed router `Harness` (tests / keyless stub). For per-tenant live routing use
    /// `live`/`live_recall`/`live_with_tools` instead, which rebuild the harness per call.
    pub fn new(harness: Harness) -> Self {
        Self { source: HarnessSource::Prebuilt(harness), tools: Vec::new(), recall_deps: None }
    }

    /// Construct from a harness AND a tool set the model may author (recall_search / propose_*). The
    /// tools are advertised but stub-executed (used where the live dispatch wiring isn't supplied).
    pub fn with_tools(harness: Harness, tools: Vec<ToolDef>) -> Self {
        Self { source: HarnessSource::Prebuilt(harness), tools, recall_deps: None }
    }

    /// Construct the AGENTIC recall seam: advertise the read-only recall tools (+ live-internet tools) AND
    /// execute them for real against the tenant's captures/wiki. `vault_root_base` is `<data_root>/objects`.
    pub fn with_recall_tools(harness: Harness, pool: PgPool, vault_root_base: PathBuf) -> Self {
        let (tools, web) = recall_tools_and_web();
        Self {
            source: HarnessSource::Prebuilt(harness),
            tools,
            recall_deps: Some(RecallToolDeps { pool, vault_root_base, web }),
        }
    }

    /// A keyless stub-backed RouterWikiLlm that always returns `reply` (for the seam's own tests).
    pub fn stub(reply: &str) -> Self {
        Self::new(Harness::with_stub(StubProvider::new(router::stub::StubScript::text(reply))))
    }

    /// The production wiki/extraction seam: per-tenant live dispatch (or the keyless stub when
    /// `QCUE_STUB_LLM=1`). `kms` decrypts the BYOK key; `pool` is the `qcue_app` pool the resolver +
    /// the per-tenant route lookup read from. The harness is rebuilt per call so each turn routes to
    /// the calling tenant's configured provider/model (S3 — fixes the tenant-blind shared harness).
    pub fn live(pool: PgPool, kms: Arc<dyn Kms + Send + Sync>, stub_reply: &str) -> Self {
        Self {
            source: HarnessSource::PerTenant { pool, kms, stub_reply: stub_reply.to_string() },
            tools: Vec::new(),
            recall_deps: None,
        }
    }

    /// The production AGENTIC recall seam: per-tenant live dispatch that advertises + REALLY executes
    /// the read-only recall tools (the model authors `recall_search` over the tenant's captures).
    pub fn live_recall(
        pool: PgPool,
        kms: Arc<dyn Kms + Send + Sync>,
        vault_root_base: PathBuf,
        stub_reply: &str,
    ) -> Self {
        let (tools, web) = recall_tools_and_web();
        Self {
            source: HarnessSource::PerTenant { pool: pool.clone(), kms, stub_reply: stub_reply.to_string() },
            tools,
            recall_deps: Some(RecallToolDeps { pool, vault_root_base, web }),
        }
    }

    /// The production Dream seam: per-tenant live dispatch advertising a caller-supplied tool surface
    /// (the `propose_*`/`read_*` set). Per-tenant so a Dream turn also targets the tenant's provider.
    pub fn live_with_tools(
        pool: PgPool,
        kms: Arc<dyn Kms + Send + Sync>,
        tools: Vec<ToolDef>,
        stub_reply: &str,
    ) -> Self {
        Self {
            source: HarnessSource::PerTenant { pool, kms, stub_reply: stub_reply.to_string() },
            tools,
            recall_deps: None,
        }
    }
}

impl RouterWikiLlm {
    /// The shared turn driver. Builds the System-prefixed history + tenant-bound tools, resolves the
    /// per-tenant harness, runs the ONE turn loop with the optional observer `sink`, and accrues cost
    /// exactly once. Both `WikiLlm` entry points delegate here so the cost-accrual site stays single.
    async fn drive(
        &self,
        t: TenantId,
        req: WikiReq,
        sink: Option<Arc<dyn TurnEventSink>>,
        over: Option<RecallOverride>,
    ) -> Result<WikiResp, WikiLlmError> {
        let over = over.unwrap_or_default();
        // F-13 — carry the per-request params through to the provider instead of dropping them with
        // `ReqParams::default()`. The structured-output schema is wrapped in the canonical
        // `{type:json_schema, json_schema:{name,schema}}` shape BOTH transports read (ChatCompletions
        // sends it verbatim; the Anthropic transport reads `json_schema.schema`). Captured before `req`
        // is consumed below. (`disable_thinking`/`cache_breakpoint` have no ReqParams home yet — P2 hooks.)
        let req_max_tokens = req.max_tokens;
        let req_response_format = req.response_format.as_ref().map(|js| {
            serde_json::json!({
                "type": "json_schema",
                "json_schema": { "name": js.name, "schema": js.schema }
            })
        });
        // F-2 — disable_thinking maps to a Minimal reasoning effort (the DeepSeek/Kimi hooks turn that into
        // `thinking:{disabled}`); recall (disable_thinking:false) leaves it at the provider default.
        // v0.2.2 — an explicit per-recall effort override (the picker) wins over both: it sets the
        // reasoning effort directly. An unknown/empty token is ignored (falls back to the rule above).
        let effort_override = over.effort.as_deref().and_then(crate::dispatch::parse_effort);
        let req_reasoning = match effort_override {
            Some(e) => Some(providers::hooks::ReasoningConfig { effort: Some(e) }),
            None => req.disable_thinking.then_some(providers::hooks::ReasoningConfig {
                effort: Some(providers::hooks::Effort::Minimal),
            }),
        };
        // The stable system prefix is a System message (cache-safe head); the untrusted content is the
        // req.messages tail (already fenced by the caller). The turn loop re-applies role-repair.
        let mut history = Vec::with_capacity(req.messages.len() + 1);
        history.push(Message {
            role: Role::System,
            content: Some(req.system.stable_prefix),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            finish_reason: None,
            reasoning: None,
            provider_data: None,
            active: true,
            is_untrusted: false,
        });
        history.extend(req.messages);
        // The dispatcher advertises this seam's tools (empty for plain extraction; recall_search /
        // propose_* for the recall/Dream seams). For the agentic recall seam we ALSO inject a
        // tenant-bound `RecallToolExec` so the model's tool calls EXECUTE for real (RLS-scoped search);
        // otherwise the tools are advertised but stub-executed.
        let tools = if self.tools.is_empty() {
            ToolDispatcher::empty()
        } else if let Some(deps) = &self.recall_deps {
            let vault_root = deps.vault_root_base.join(format!("t/{t}/u/_"));
            let exec: Arc<dyn ToolExec> = Arc::new(crate::recall_tools::RecallToolExec::new(
                t,
                deps.pool.clone(),
                vault_root,
                None, // a user recall query has no in-flight session to exclude (A-R24 is for Dream)
                deps.web.clone(), // the shared live-internet client (None → web tools disabled)
            ));
            ToolDispatcher::with_handler(self.tools.clone(), exec)
        } else {
            ToolDispatcher::with_defs(self.tools.clone())
        };
        let ctx = TurnContext {
            history,
            budget: turn_budget_from_env(),
            params: router::transport::ReqParams {
                max_tokens: Some(req_max_tokens),
                response_format: req_response_format,
                reasoning: req_reasoning,
                ..Default::default()
            },
            persistence: Box::new(NoPersist),
            cost_guard: Box::new(AllowCost),
            tools,
            tenant: t,
            sink,
        };
        // Resolve the harness for THIS tenant: a fixed one (stub/tests) or a per-tenant live build whose
        // FallbackChain is rooted at the tenant's configured provider/model (not a boot-time env default).
        // For the live path we also capture (pool, provider, model) to price + accrue the turn's usage.
        let per_tenant_harness;
        let mut accrual: Option<(PgPool, String, String)> = None;
        let harness: &Harness = match &self.source {
            HarnessSource::Prebuilt(h) => h,
            HarnessSource::PerTenant { pool, kms, stub_reply } => {
                // v0.2.2 — an explicit (provider, model) override (the recall picker) roots the harness
                // there instead of the tenant's default active model; otherwise the per-tenant route.
                let (h, provider, model) = match over.route() {
                    Some((p, m)) => {
                        crate::dispatch::build_harness_for_route(
                            pool.clone(), kms.clone(), t, &p, &m, stub_reply,
                        )
                        .await
                    }
                    None => {
                        crate::dispatch::build_harness_for(pool.clone(), kms.clone(), t, stub_reply).await
                    }
                };
                per_tenant_harness = h;
                accrual = Some((pool.clone(), provider, model));
                &per_tenant_harness
            }
        };
        match run_turn(harness, ctx, CancellationToken::new())
            .await
            .map_err(WikiLlmError::Provider)?
        {
            TurnResult::Final { content, usage, finish_reason } => {
                // Single cost-accrual site: every wiki/recall/dream provider call funnels through here.
                if let Some((pool, provider, model)) = &accrual {
                    crate::dispatch::accrue_turn_cost(pool, t, provider, model, &usage).await;
                }
                Ok(WikiResp {
                    content: content.unwrap_or_default(),
                    usage: Some(usage),
                    truncated: finish_reason == protocol::FinishReason::Length,
                })
            }
            TurnResult::Interrupted => Err(WikiLlmError::Cancelled),
            TurnResult::Refused(r) => Err(WikiLlmError::Provider(r)),
        }
    }
}

#[async_trait]
impl WikiLlm for RouterWikiLlm {
    async fn create_message(&self, t: TenantId, req: WikiReq) -> Result<WikiResp, WikiLlmError> {
        self.drive(t, req, None, None).await
    }
    /// The agentic recall / WSS entry: thread the per-request observer into the turn loop.
    async fn create_message_observed(
        &self,
        t: TenantId,
        req: WikiReq,
        sink: Option<Arc<dyn TurnEventSink>>,
    ) -> Result<WikiResp, WikiLlmError> {
        self.drive(t, req, sink, None).await
    }
    /// v0.2.2 — the recall picker entry: thread the per-request route + effort override into the turn.
    async fn create_message_observed_with_override(
        &self,
        t: TenantId,
        req: WikiReq,
        sink: Option<Arc<dyn TurnEventSink>>,
        over: RecallOverride,
    ) -> Result<WikiResp, WikiLlmError> {
        self.drive(t, req, sink, Some(over)).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use protocol::Role;

    #[tokio::test]
    async fn router_wiki_llm_maps_final_to_wiki_resp() {
        let llm = RouterWikiLlm::stub(r#"{"fully_redundant":false}"#);
        let req = WikiReq {
            system: wiki::llm::SystemBlocks { stable_prefix: "SYS".into() },
            messages: vec![Message {
                role: Role::User,
                content: Some("hi".into()),
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
            max_tokens: 64,
            cache_breakpoint: Some(1),
            disable_thinking: true,
        };
        let resp = llm.create_message(uuid::Uuid::now_v7(), req).await.unwrap();
        assert_eq!(resp.content, r#"{"fully_redundant":false}"#);
    }

    // F-13 — the live driver used to build the turn with `params: Default::default()`, silently dropping
    // every WikiReq per-request param (max_tokens, response_format). A recording dispatch proves they now
    // reach the provider.
    struct RecordingDispatch {
        seen: std::sync::Arc<std::sync::Mutex<Option<router::transport::ReqParams>>>,
    }
    #[async_trait::async_trait]
    impl router::dispatch::ProviderDispatch for RecordingDispatch {
        async fn complete(
            &self,
            req: &router::dispatch::DispatchRequest,
            _cancel: CancellationToken,
        ) -> Result<protocol::NormalizedResponse, protocol::ApiError> {
            *self.seen.lock().unwrap() = Some(req.params.clone());
            Ok(protocol::NormalizedResponse {
                content: Some("{}".into()),
                tool_calls: None,
                finish_reason: protocol::FinishReason::Stop,
                reasoning: None,
                usage: None,
                provider_data: None,
            })
        }
    }

    #[tokio::test]
    async fn drive_passes_wikireq_params_to_the_provider() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
        let llm = RouterWikiLlm::new(Harness::with_dispatch(Box::new(RecordingDispatch {
            seen: seen.clone(),
        })));
        let req = WikiReq {
            system: wiki::llm::SystemBlocks { stable_prefix: "SYS".into() },
            messages: vec![Message {
                role: Role::User,
                content: Some("hi".into()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
                finish_reason: None,
                reasoning: None,
                provider_data: None,
                active: true,
                is_untrusted: true,
            }],
            response_format: Some(wiki::llm::JsonSchema {
                name: "Out".into(),
                schema: serde_json::json!({"type": "object"}),
            }),
            max_tokens: 1234,
            cache_breakpoint: Some(1),
            disable_thinking: true,
        };
        llm.create_message(uuid::Uuid::now_v7(), req).await.unwrap();
        let params = seen.lock().unwrap().clone().expect("the provider must have been called");
        assert_eq!(params.max_tokens, Some(1234), "WikiReq.max_tokens must reach the provider");
        let rf = params.response_format.expect("WikiReq.response_format must reach the provider");
        // The canonical json_schema shape both transports read (`json_schema.schema`).
        assert_eq!(rf["json_schema"]["schema"]["type"], "object", "response_format shape: {rf}");
    }

    // F-13/F-2 — WikiReq.disable_thinking must map to a Minimal reasoning effort so the provider hooks
    // (DeepSeek/Kimi) actually disable thinking on the live path (ingest sets disable_thinking:true).
    #[tokio::test]
    async fn drive_maps_disable_thinking_to_minimal_reasoning() {
        async fn captured(disable_thinking: bool) -> Option<providers::hooks::ReasoningConfig> {
            let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
            let llm = RouterWikiLlm::new(Harness::with_dispatch(Box::new(RecordingDispatch {
                seen: seen.clone(),
            })));
            let req = WikiReq {
                system: wiki::llm::SystemBlocks { stable_prefix: "SYS".into() },
                messages: vec![Message {
                    role: Role::User,
                    content: Some("hi".into()),
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
                max_tokens: 64,
                cache_breakpoint: None,
                disable_thinking,
            };
            llm.create_message(uuid::Uuid::now_v7(), req).await.unwrap();
            let p = seen.lock().unwrap().clone().unwrap();
            p.reasoning
        }
        assert_eq!(
            captured(true).await.and_then(|r| r.effort),
            Some(providers::hooks::Effort::Minimal),
            "disable_thinking:true must map to Minimal reasoning effort",
        );
        assert!(captured(false).await.is_none(), "disable_thinking:false leaves reasoning unset");
    }

    #[test]
    fn turn_budget_reads_env_with_safe_defaults() {
        // SAFETY: single-threaded test; set + clear the env around the call.
        unsafe { std::env::remove_var("QCUE_TURN_TOKEN_BUDGET"); }
        unsafe { std::env::remove_var("QCUE_TURN_MAX_ROUNDS"); }
        let b = super::turn_budget_from_env();
        assert!(!b.exhausted(), "fresh budget is not exhausted");
        // override is honored
        unsafe { std::env::set_var("QCUE_TURN_MAX_ROUNDS", "1"); }
        let mut b2 = super::turn_budget_from_env();
        b2.tick_round();
        assert!(b2.exhausted(), "max-rounds=1 → exhausted after one round");
        unsafe { std::env::remove_var("QCUE_TURN_MAX_ROUNDS"); }
    }
}
