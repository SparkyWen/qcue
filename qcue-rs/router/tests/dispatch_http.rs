#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue §5.2 — HttpDispatch wire-shape + recovery tests (wiremock). Asserts the outbound body shape
// and auth header per api_mode, plus a 429→rotate and a 5xx→fallback path via a fake resolver.
use async_trait::async_trait;
use protocol::{ApiMode, CredStatus, Message, Role};
use providers::hooks::DefaultHooks;
use providers::profile::{AuthType, ProviderProfile, TempPolicy};
use providers::registry::Registry;
use router::dispatch::{DispatchRequest, ProviderDispatch};
use router::dispatch_http::{build_outbound_body, HttpDispatch};
use router::pool::{CredentialPool, PoolStrategy, PooledCredential};
use router::resolver::{CredentialResolver, ResolveError};
use router::retry_loop::FallbackChain;
use router::transport::{ReqParams, ServerTool};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const TEST_KEY: &str = "sk-test-1234";
const TENANT_AAD: &str = "tenant-A";

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

/// A fake resolver: one Ok credential per provider; decrypt round-trips a known key via the secrets
/// crate (StubKms seal → decrypt_with_tenant) so we exercise the real ZeroizingKey path.
struct FakeResolver {
    cred_id: Uuid,
    key_hint: String,
}
impl FakeResolver {
    fn new() -> Self {
        Self { cred_id: Uuid::nil(), key_hint: "hint-A".into() }
    }
}
#[async_trait]
impl CredentialResolver for FakeResolver {
    async fn pool_for(
        &self,
        _tenant: Uuid,
        _provider: &str,
    ) -> Result<CredentialPool, ResolveError> {
        let cred = PooledCredential {
            id: self.cred_id,
            label: None,
            priority: 0,
            status: CredStatus::Ok,
            key_hint: self.key_hint.clone(),
            last_error_code: None,
            last_error_reason: None,
            request_count: 0,
        };
        Ok(CredentialPool::new(vec![cred], PoolStrategy::FillFirst))
    }
    async fn decrypt(
        &self,
        _tenant: Uuid,
        _cred_id: Uuid,
    ) -> Result<secrets::ZeroizingKey, ResolveError> {
        let kms = secrets::StubKms::new();
        let enc = secrets::EncryptedCredential::seal(&kms, TEST_KEY.as_bytes(), TENANT_AAD)
            .map_err(|e| ResolveError::Decrypt(e.to_string()))?;
        secrets::decrypt_with_tenant(&kms, &enc, TENANT_AAD)
            .map_err(|e| ResolveError::Decrypt(e.to_string()))
    }
}

fn chat_profile(base_url: &str) -> ProviderProfile {
    let mut env_http_headers = HashMap::new();
    env_http_headers.insert("Authorization".into(), "OPENAI_API_KEY".into());
    ProviderProfile {
        name: "openai".into(),
        api_mode: ApiMode::ChatCompletions,
        base_url: base_url.into(),
        models_url: None,
        auth_type: AuthType::ApiKey,
        default_headers: Default::default(),
        env_http_headers,
        fixed_temperature: TempPolicy::Inherit,
        default_max_tokens: Some(256),
        fallback_models: vec![],
        supports_vision: true,
        request_max_retries: 3,
        stream_idle_timeout_ms: 30_000,
        stream_ttfb_timeout_ms: 60_000,
        cache_supported: true,
        hooks: Box::new(DefaultHooks),
    }
}

fn anthropic_profile(base_url: &str) -> ProviderProfile {
    let mut default_headers = HashMap::new();
    default_headers.insert("anthropic-version".into(), "2023-06-01".into());
    let mut env_http_headers = HashMap::new();
    env_http_headers.insert("x-api-key".into(), "ANTHROPIC_API_KEY".into());
    ProviderProfile {
        name: "anthropic".into(),
        api_mode: ApiMode::AnthropicMessages,
        base_url: base_url.into(),
        models_url: None,
        auth_type: AuthType::ApiKey,
        default_headers,
        env_http_headers,
        fixed_temperature: TempPolicy::Inherit,
        default_max_tokens: Some(256),
        fallback_models: vec![],
        supports_vision: true,
        request_max_retries: 3,
        stream_idle_timeout_ms: 30_000,
        stream_ttfb_timeout_ms: 90_000,
        cache_supported: true,
        hooks: Box::new(DefaultHooks),
    }
}

/// A spy hooks impl that touches the four request-shaping hooks so a test can prove each is invoked by
/// `build_outbound_body` (they were previously dead code — F-2).
struct SpyHooks;
#[async_trait]
impl providers::hooks::ProviderHooks for SpyHooks {
    fn prepare_messages(&self, mut m: Vec<Message>) -> Vec<Message> {
        if let Some(last) = m.last_mut() {
            last.content = Some(format!("{}::PREP", last.content.clone().unwrap_or_default()));
        }
        m
    }
    fn build_extra_body(&self, _s: Option<&str>, _ctx: &providers::hooks::ReqCtx) -> serde_json::Value {
        serde_json::json!({ "xb": 1 })
    }
    fn build_api_kwargs_extras(
        &self,
        _r: Option<&providers::hooks::ReasoningConfig>,
        _ctx: &providers::hooks::ReqCtx,
    ) -> (serde_json::Value, serde_json::Map<String, serde_json::Value>) {
        let mut top = serde_json::Map::new();
        top.insert("top_marker".into(), serde_json::json!(3));
        (serde_json::json!({ "xb2": 2 }), top)
    }
    fn get_max_tokens(&self, _model: &str) -> Option<u32> {
        Some(99)
    }
}

#[test]
fn hooks_are_wired_into_build_outbound_body() {
    // F-2 — build_outbound_body must run the per-provider hooks: prepare_messages, build_extra_body,
    // build_api_kwargs_extras (extra_body + top-level), and get_max_tokens as the max_tokens fallback.
    let mut profile = chat_profile("http://x");
    profile.hooks = Box::new(SpyHooks);
    profile.default_max_tokens = None; // so get_max_tokens(99) is the only max_tokens source
    let body = build_outbound_body(&profile, "gpt-4o", &[user("hi")], &[], &ReqParams::default());
    assert_eq!(body["messages"][0]["content"], "hi::PREP", "prepare_messages must run: {body}");
    assert_eq!(body["xb"], 1, "build_extra_body must be merged: {body}");
    assert_eq!(body["xb2"], 2, "build_api_kwargs_extras extra_body must be merged: {body}");
    assert_eq!(body["top_marker"], 3, "build_api_kwargs_extras top-level must be merged: {body}");
    assert_eq!(body["max_tokens"], 99, "get_max_tokens must supply the fallback cap: {body}");
}

#[test]
fn build_outbound_body_renders_web_search_server_tool() {
    // F-1 — the live dispatch call site (build_outbound_body) must render a provider-native server tool
    // end-to-end onto the wire body, not just the transport in isolation.
    let reg = providers::registry::register_all();
    let profile = reg.get("anthropic").unwrap();
    let params = ReqParams {
        server_tools: vec![ServerTool::WebSearch { max_uses: None }],
        ..Default::default()
    };
    let body = build_outbound_body(profile, "claude-opus-4-8", &[user("hi")], &[], &params);
    let has_web_search = body["tools"]
        .as_array()
        .map(|a| a.iter().any(|t| t.get("type").and_then(|x| x.as_str()) == Some("web_search_20260209")))
        .unwrap_or(false);
    assert!(has_web_search, "the live dispatch path must render the web_search server tool: {body}");
}

#[test]
fn deepseek_reasoning_effort_minimal_disables_thinking() {
    // F-2 — the DeepSeek apply_reasoning_effort hook (Minimal → thinking:disabled) must actually fire.
    let reg = providers::registry::register_all();
    let profile = reg.get("deepseek").unwrap();
    let params = ReqParams {
        reasoning: Some(providers::hooks::ReasoningConfig {
            effort: Some(providers::hooks::Effort::Minimal),
        }),
        ..Default::default()
    };
    let body = build_outbound_body(profile, "deepseek-chat", &[user("hi")], &[], &params);
    assert_eq!(body["thinking"]["type"], "disabled", "apply_reasoning_effort must run: {body}");
}

#[test]
fn gpt5x_routes_responses_body_with_reasoning_and_tools() {
    // RESP-R2/R5 — the PROPER fix (supersedes the chat stop-gap): a gpt-5.x turn now routes to the
    // /v1/responses transport, where reasoning effort + function tools coexist. The live dispatch body for
    // gpt-5.5 carries a NATIVE `reasoning:{effort}` object + FLAT tools + max_output_tokens, and NOT the
    // chat `reasoning_effort` key that 400'd. gpt-4o stays on chat/completions (nested tools, no reasoning
    // object). The chat-suppression stop-gap stays unit-tested in providers/tests/quirks.rs.
    let reg = providers::registry::register_all();
    let profile = reg.get("openai").unwrap();
    let tool = protocol::ToolDef {
        name: "web_search".into(),
        description: "search the web".into(),
        input_schema: serde_json::json!({"type":"object","properties":{},"required":[]}),
    };
    let params = ReqParams {
        max_tokens: Some(2048),
        reasoning: Some(providers::hooks::ReasoningConfig {
            effort: Some(providers::hooks::Effort::High),
        }),
        ..Default::default()
    };
    let body = build_outbound_body(profile, "gpt-5.5", &[user("hi")], std::slice::from_ref(&tool), &params);
    assert_eq!(body["reasoning"]["effort"], "high", "gpt-5.5 carries the native Responses reasoning object: {body}");
    assert!(body.get("reasoning_effort").is_none(), "must NOT use the chat reasoning_effort key: {body}");
    assert_eq!(body["tools"][0]["type"], "function");
    assert!(body["tools"][0].get("function").is_none(), "Responses tools are FLAT: {body}");
    assert_eq!(body["tool_choice"], "auto");
    assert_eq!(body["max_output_tokens"], 2048, "Responses uses max_output_tokens: {body}");
    assert!(body.get("max_completion_tokens").is_none() && body.get("max_tokens").is_none());

    // Contrast: gpt-4o stays on chat/completions — nested tools, no Responses reasoning object.
    let body4o = build_outbound_body(profile, "gpt-4o", &[user("hi")], std::slice::from_ref(&tool), &params);
    assert!(body4o.get("reasoning").is_none(), "gpt-4o stays on chat (no Responses reasoning object): {body4o}");
    assert!(body4o["tools"][0].get("function").is_some(), "chat tools are NESTED: {body4o}");
}

fn dispatch_request() -> DispatchRequest {
    DispatchRequest {
        messages: vec![user("hello")],
        tools: vec![],
        params: ReqParams::default(),
        tenant: Uuid::nil(),
    }
}

fn insecure_client() -> reqwest::Client {
    reqwest::Client::builder().build().unwrap()
}

#[tokio::test]
async fn chat_completions_outbound_body_and_authorization_header() {
    let server = MockServer::start().await;
    // The base_url already ends in /v1; versioned_base_url joins `chat/completions`.
    let base = format!("{}/v1", server.uri());

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", format!("Bearer {TEST_KEY}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [ { "message": { "role": "assistant", "content": "hi back" }, "finish_reason": "stop" } ],
            "usage": { "prompt_tokens": 3, "completion_tokens": 2 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut reg = Registry::from_profiles(HashMap::new());
    reg.insert("openai", chat_profile(&base));
    let resolver = Arc::new(FakeResolver::new());
    let chain = FallbackChain::new(vec![(
        "openai".into(),
        "gpt-4o-mini".into(),
        ApiMode::ChatCompletions,
    )]);
    let dispatch = HttpDispatch::new(insecure_client(), Arc::new(reg), resolver, chain, true);

    let nr = dispatch
        .complete(&dispatch_request(), CancellationToken::new())
        .await
        .expect("chat completion succeeds");
    assert_eq!(nr.content.as_deref(), Some("hi back"));

    // Assert the outbound body had the ChatCompletions shape (model + messages array).
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let body: serde_json::Value = reqs[0].body_json().unwrap();
    assert_eq!(body["model"], "gpt-4o-mini");
    assert!(body["messages"].is_array(), "messages must be an array: {body}");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "hello");
}

#[tokio::test]
async fn anthropic_messages_outbound_body_and_x_api_key_header() {
    let server = MockServer::start().await;
    let base = format!("{}/v1", server.uri());

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", TEST_KEY))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [ { "type": "text", "text": "claude says hi" } ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 3, "output_tokens": 2 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut reg = Registry::from_profiles(HashMap::new());
    reg.insert("anthropic", anthropic_profile(&base));
    let resolver = Arc::new(FakeResolver::new());
    let chain = FallbackChain::new(vec![(
        "anthropic".into(),
        "claude-sonnet-4".into(),
        ApiMode::AnthropicMessages,
    )]);
    let dispatch = HttpDispatch::new(insecure_client(), Arc::new(reg), resolver, chain, true);

    let nr = dispatch
        .complete(&dispatch_request(), CancellationToken::new())
        .await
        .expect("anthropic completion succeeds");
    assert_eq!(nr.content.as_deref(), Some("claude says hi"));

    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let body: serde_json::Value = reqs[0].body_json().unwrap();
    assert_eq!(body["model"], "claude-sonnet-4");
    assert!(body.get("max_tokens").is_some(), "anthropic body must carry max_tokens: {body}");
    assert!(body["messages"].is_array(), "messages must be an array: {body}");
    // cache_control is applied to the last non-system message (cache_supported=true) — and Anthropic
    // ONLY permits cache_control on a CONTENT BLOCK, never as a top-level field of the message object
    // (a top-level cache_control is a 400 "messages.N.cache_control: Extra inputs are not permitted").
    let msgs = body["messages"].as_array().unwrap();
    assert!(
        msgs.iter().all(|m| m.get("cache_control").is_none()),
        "cache_control must NOT be a top-level message field: {body}"
    );
    let last = msgs.last().unwrap();
    let blocks = last["content"].as_array().expect("anthropic message content must be a block array");
    assert!(
        blocks.iter().any(|b| b.get("cache_control").is_some()),
        "cache_control must sit on a content block: {body}"
    );
}

#[test]
fn openai_gpt5_and_o_series_use_max_completion_tokens() {
    // OpenAI's gpt-5.x and o-series reject `max_tokens` ("Use 'max_completion_tokens' instead", 400).
    // The ChatCompletions transport must pick the param name by model; classic models keep max_tokens.
    let profile = chat_profile("http://example/v1"); // openai chat profile, default_max_tokens=256
    let msgs = vec![user("hi")];
    let p = ReqParams::default();
    for m in ["gpt-5.1", "gpt-5.1-mini", "o4-mini"] {
        let body = build_outbound_body(&profile, m, &msgs, &[], &p);
        assert!(body.get("max_completion_tokens").is_some(), "{m} must use max_completion_tokens: {body}");
        assert!(body.get("max_tokens").is_none(), "{m} must NOT send max_tokens: {body}");
    }
    for m in ["gpt-4o-mini", "deepseek-chat"] {
        let body = build_outbound_body(&profile, m, &msgs, &[], &p);
        assert!(body.get("max_tokens").is_some(), "{m} must keep max_tokens: {body}");
        assert!(body.get("max_completion_tokens").is_none(), "{m} must not use max_completion_tokens: {body}");
    }
}

/// Two-credential pool: first 429 marks the selected cred Exhausted and rotates to the second,
/// which then succeeds. Same provider link (rotate, not fallback).
struct RotateResolver {
    rotations: Arc<AtomicUsize>,
}
#[async_trait]
impl CredentialResolver for RotateResolver {
    async fn pool_for(
        &self,
        _tenant: Uuid,
        _provider: &str,
    ) -> Result<CredentialPool, ResolveError> {
        let creds = vec![
            PooledCredential {
                id: Uuid::nil(),
                label: None,
                priority: 0,
                status: CredStatus::Ok,
                key_hint: "hint-A".into(),
                last_error_code: None,
                last_error_reason: None,
                request_count: 0,
            },
            PooledCredential {
                id: Uuid::nil(),
                label: None,
                priority: 1,
                status: CredStatus::Ok,
                key_hint: "hint-B".into(),
                last_error_code: None,
                last_error_reason: None,
                request_count: 0,
            },
        ];
        Ok(CredentialPool::new(creds, PoolStrategy::FillFirst))
    }
    async fn decrypt(
        &self,
        _tenant: Uuid,
        _cred_id: Uuid,
    ) -> Result<secrets::ZeroizingKey, ResolveError> {
        let kms = secrets::StubKms::new();
        let enc = secrets::EncryptedCredential::seal(&kms, TEST_KEY.as_bytes(), TENANT_AAD)
            .map_err(|e| ResolveError::Decrypt(e.to_string()))?;
        secrets::decrypt_with_tenant(&kms, &enc, TENANT_AAD)
            .map_err(|e| ResolveError::Decrypt(e.to_string()))
    }
    async fn persist_transitions(
        &self,
        _tenant: Uuid,
        _provider: &str,
        _pool: &CredentialPool,
    ) -> Result<(), ResolveError> {
        self.rotations.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn rate_limit_429_rotates_credential_then_succeeds() {
    let server = MockServer::start().await;
    let base = format!("{}/v1", server.uri());
    let hits = Arc::new(AtomicUsize::new(0));

    // First call → 429 (rate limit with a transient signal); second call → 200.
    let hits_for_mock = hits.clone();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |_req: &Request| {
            let n = hits_for_mock.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                ResponseTemplate::new(429).set_body_json(serde_json::json!({
                    "error": { "message": "rate limit exceeded, try again in 1s" }
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "choices": [ { "message": { "role": "assistant", "content": "after rotate" }, "finish_reason": "stop" } ]
                }))
            }
        })
        .mount(&server)
        .await;

    let mut reg = Registry::from_profiles(HashMap::new());
    reg.insert("openai", chat_profile(&base));
    let rotations = Arc::new(AtomicUsize::new(0));
    let resolver = Arc::new(RotateResolver { rotations: rotations.clone() });
    let chain = FallbackChain::new(vec![(
        "openai".into(),
        "gpt-4o-mini".into(),
        ApiMode::ChatCompletions,
    )]);
    let dispatch = HttpDispatch::new(insecure_client(), Arc::new(reg), resolver, chain, true);

    let nr = dispatch
        .complete(&dispatch_request(), CancellationToken::new())
        .await
        .expect("succeeds after rotate");
    assert_eq!(nr.content.as_deref(), Some("after rotate"));
    assert_eq!(hits.load(Ordering::SeqCst), 2, "exactly one retry after the 429");
    assert!(rotations.load(Ordering::SeqCst) >= 1, "a rotate persist_transitions fired");
}

#[tokio::test]
async fn server_error_5xx_falls_back_to_next_provider() {
    let primary = MockServer::start().await;
    let secondary = MockServer::start().await;
    let primary_base = format!("{}/v1", primary.uri());
    let secondary_base = format!("{}/v1", secondary.uri());

    // primary always 500 → ServerError → Fallback.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream boom"))
        .mount(&primary)
        .await;
    // secondary 200.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [ { "message": { "role": "assistant", "content": "secondary ok" }, "finish_reason": "stop" } ]
        })))
        .expect(1)
        .mount(&secondary)
        .await;

    let mut reg = Registry::from_profiles(HashMap::new());
    reg.insert("openai", chat_profile(&primary_base));
    reg.insert("backup", chat_profile(&secondary_base));
    let resolver = Arc::new(FakeResolver::new());
    let chain = FallbackChain::new(vec![
        ("openai".into(), "gpt-4o-mini".into(), ApiMode::ChatCompletions),
        ("backup".into(), "gpt-4o-mini".into(), ApiMode::ChatCompletions),
    ]);
    let dispatch = HttpDispatch::new(insecure_client(), Arc::new(reg), resolver, chain, true);

    let nr = dispatch
        .complete(&dispatch_request(), CancellationToken::new())
        .await
        .expect("falls back to secondary");
    assert_eq!(nr.content.as_deref(), Some("secondary ok"));
}
