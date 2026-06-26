#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R13/R74 — DeepSeek end-to-end harness wire test. Drives the REAL `deepseek::profile()`
// (the one register_all() ships) through HttpDispatch so we prove the harness speaks DeepSeek's
// actual OpenAI-compatible wire: Bearer auth on the Authorization header, the /v1/chat/completions
// URL join, a tool (recall_search) serialized in the request, and — on the response — DeepSeek's
// signature `reasoning_content` + `tool_calls` normalized into one internal shape (so agentic recall
// can drive a tool→answer loop on DeepSeek exactly as it does on OpenAI/Anthropic).
//
// Plus `deepseek_live_smoke`: an env-gated REAL call to api.deepseek.com (skips when DEEPSEEK_API_KEY
// is unset, mirroring the QCUE_TEST_REDIS pattern) — the ground-truth "the vendor API actually runs".
use async_trait::async_trait;
use protocol::{ApiMode, CredStatus, FinishReason, Message, Role, ToolDef};
use providers::registry::Registry;
use router::dispatch::{DispatchRequest, ProviderDispatch};
use router::dispatch_http::HttpDispatch;
use router::pool::{CredentialPool, PoolStrategy, PooledCredential};
use router::resolver::{CredentialResolver, ResolveError};
use router::retry_loop::FallbackChain;
use router::transport::ReqParams;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

fn recall_tool() -> ToolDef {
    ToolDef {
        name: "recall_search".into(),
        description: "Search the user's notes for relevant context.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
            "required": ["query"]
        }),
    }
}

/// A resolver that round-trips a fixed plaintext key through the real ZeroizingKey path.
struct FixedKeyResolver {
    key: String,
}
#[async_trait]
impl CredentialResolver for FixedKeyResolver {
    async fn pool_for(&self, _t: Uuid, _p: &str) -> Result<CredentialPool, ResolveError> {
        let cred = PooledCredential {
            id: Uuid::nil(),
            label: None,
            priority: 0,
            status: CredStatus::Ok,
            key_hint: "hint".into(),
            last_error_code: None,
            last_error_reason: None,
            request_count: 0,
        };
        Ok(CredentialPool::new(vec![cred], PoolStrategy::FillFirst))
    }
    async fn decrypt(&self, _t: Uuid, _c: Uuid) -> Result<secrets::ZeroizingKey, ResolveError> {
        let kms = secrets::StubKms::new();
        let enc = secrets::EncryptedCredential::seal(&kms, self.key.as_bytes(), TENANT_AAD)
            .map_err(|e| ResolveError::Decrypt(e.to_string()))?;
        secrets::decrypt_with_tenant(&kms, &enc, TENANT_AAD)
            .map_err(|e| ResolveError::Decrypt(e.to_string()))
    }
}

/// Build a registry whose `deepseek` profile is the REAL one, but pointed at `base_url`.
fn deepseek_registry(base_url: &str) -> Registry {
    let mut profile = providers::vendors::deepseek::profile();
    profile.base_url = base_url.to_string();
    let mut reg = Registry::from_profiles(HashMap::new());
    reg.insert("deepseek", profile);
    reg
}

/// The DeepSeek profile drives a full tool-calling + reasoning turn over its real wire shape.
#[tokio::test]
async fn deepseek_profile_drives_tool_calling_and_reasoning_wire() {
    let server = MockServer::start().await;

    // DeepSeek's base_url is https://api.deepseek.com (no /v1); versioned_base_url appends /v1, so the
    // real endpoint is /v1/chat/completions. Mounting there proves the URL join matches production.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", format!("Bearer {TEST_KEY}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    // DeepSeek's signature reasoning field (Q7) — must normalize into `reasoning`.
                    "reasoning_content": "The user wants their notes; I'll call recall_search.",
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": { "name": "recall_search", "arguments": "{\"query\": \"deepseek\"}" }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 20, "completion_tokens": 8 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let reg = deepseek_registry(&server.uri());
    let resolver = Arc::new(FixedKeyResolver { key: TEST_KEY.into() });
    // deepseek-chat is DeepSeek's tool-capable model (the one agentic recall must use).
    let chain = FallbackChain::new(vec![(
        "deepseek".into(),
        "deepseek-chat".into(),
        ApiMode::ChatCompletions,
    )]);
    let dispatch = HttpDispatch::new(
        reqwest::Client::builder().build().unwrap(),
        Arc::new(reg),
        resolver,
        chain,
        true,
    );

    let req = DispatchRequest {
        messages: vec![user("find my notes about deepseek")],
        tools: vec![recall_tool()],
        params: ReqParams::default(),
        tenant: Uuid::nil(),
    };
    let nr = dispatch
        .complete(&req, CancellationToken::new())
        .await
        .expect("deepseek tool-calling turn completes");

    // Response normalization: reasoning_content captured, tool_call parsed, finish_reason mapped.
    assert_eq!(nr.finish_reason, FinishReason::ToolCalls);
    assert_eq!(
        nr.reasoning.as_deref(),
        Some("The user wants their notes; I'll call recall_search.")
    );
    let calls = nr.tool_calls.expect("a tool_call was returned");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "recall_search");
    assert!(calls[0].arguments.contains("deepseek"), "args: {}", calls[0].arguments);

    // Outbound wire shape: model, messages array, and the tool advertised in OpenAI function form.
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let body: serde_json::Value = reqs[0].body_json().unwrap();
    assert_eq!(body["model"], "deepseek-chat");
    assert!(body["messages"].is_array(), "messages must be an array: {body}");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "recall_search");
    assert!(body.get("max_tokens").is_some(), "deepseek default_max_tokens must ride: {body}");
}

/// Ground truth: a REAL call to api.deepseek.com. Skips unless DEEPSEEK_API_KEY is set, so CI/keyless
/// runs stay green. Run with:  DEEPSEEK_API_KEY=sk-... cargo test -p router --test deepseek_wire -- --nocapture
#[tokio::test]
async fn deepseek_live_smoke() {
    let Ok(key) = std::env::var("DEEPSEEK_API_KEY") else {
        eprintln!("skipping deepseek_live_smoke: set DEEPSEEK_API_KEY to run the real-vendor call");
        return;
    };

    // The REAL profile (base_url = https://api.deepseek.com), no wiremock.
    let mut reg = Registry::from_profiles(HashMap::new());
    reg.insert("deepseek", providers::vendors::deepseek::profile());
    let resolver = Arc::new(FixedKeyResolver { key });
    let chain = FallbackChain::new(vec![(
        "deepseek".into(),
        "deepseek-chat".into(),
        ApiMode::ChatCompletions,
    )]);
    let dispatch = HttpDispatch::new(
        reqwest::Client::builder().build().unwrap(),
        Arc::new(reg),
        resolver,
        chain,
        false, // require https — api.deepseek.com is https
    );

    let req = DispatchRequest {
        messages: vec![user("Reply with exactly one word: pong")],
        tools: vec![recall_tool()], // advertise a tool to exercise the agentic wire end-to-end
        params: ReqParams { max_tokens: Some(32), ..Default::default() },
        tenant: Uuid::nil(),
    };
    let nr = dispatch
        .complete(&req, CancellationToken::new())
        .await
        .expect("real api.deepseek.com call succeeds (auth + wire + parse)");

    eprintln!(
        "deepseek live: content={:?} reasoning={:?} tool_calls={:?} finish={:?}",
        nr.content, nr.reasoning, nr.tool_calls, nr.finish_reason
    );
    assert!(
        nr.content.is_some() || nr.tool_calls.is_some(),
        "DeepSeek returned neither content nor tool_calls"
    );
}
