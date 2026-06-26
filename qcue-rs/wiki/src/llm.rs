// QCue S2-R1 — the ONLY LLM seam S2 sees. S1's router implements it (RouterWikiLlm); S2 never holds
// keys / builds provider bodies. A deterministic StubWikiLlm makes the whole ingest pipeline testable
// without a network or credentials. Recall (the NEXT milestone) extends this trait with `rank`/stream
// helpers; ingest needs only `create_message` now.
use async_trait::async_trait;
use futures_util::Stream;
use protocol::{CanonicalUsage, Message, StreamEvent};
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Mutex;

pub type TenantId = uuid::Uuid;
pub type StreamEventBox = Pin<Box<dyn Stream<Item = Result<StreamEvent, WikiLlmError>> + Send>>;

#[derive(Debug, thiserror::Error)]
pub enum WikiLlmError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("cost ceiling reached")]
    CostCeiling,
    #[error("cancelled")]
    Cancelled,
}

/// The stable system prefix (cache-safe; pitfall #2). Built once per op; never carries volatile bytes.
#[derive(Debug, Clone, Default)]
pub struct SystemBlocks {
    pub stable_prefix: String,
}

/// A JSON-schema for structured extraction (json_schema / prefill).
#[derive(Debug, Clone)]
pub struct JsonSchema {
    pub name: String,
    pub schema: serde_json::Value,
}

pub struct WikiReq {
    pub system: SystemBlocks,                // stable prefix (cache-safe; built once per op)
    pub messages: Vec<Message>,              // untrusted content lives ONLY in the tail (RKM §7 #3)
    pub response_format: Option<JsonSchema>,
    pub max_tokens: u32,
    pub cache_breakpoint: Option<usize>,     // static-prefix length for cache_control
    pub disable_thinking: bool,              // strip CoT preamble from JSON/markdown
}

pub struct WikiResp {
    pub content: String,
    pub usage: Option<CanonicalUsage>,
    pub truncated: bool,
}

/// A per-request route + reasoning-effort override (v0.2.2 — the recall composer's
/// model/effort picker). All-`None` means "use the tenant's default route + effort"
/// (identical to no override). Carried as plain strings so this stays in the `wiki`
/// seam crate (no `providers` coupling); `RouterWikiLlm` maps `effort` to the typed
/// `providers::hooks::Effort` and roots the harness at (`provider`,`model`). The
/// override must resolve within the tenant's configured BYOK providers (the picker
/// only offers those), so RLS/credential isolation is unchanged.
#[derive(Debug, Clone, Default)]
pub struct RecallOverride {
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Reasoning-effort wire token: `minimal|low|medium|high|xhigh|max`.
    pub effort: Option<String>,
}

impl RecallOverride {
    pub fn is_empty(&self) -> bool {
        self.provider.is_none() && self.model.is_none() && self.effort.is_none()
    }

    /// The explicit (provider, model) route, only when BOTH are set.
    pub fn route(&self) -> Option<(String, String)> {
        match (&self.provider, &self.model) {
            (Some(p), Some(m)) => Some((p.clone(), m.clone())),
            _ => None,
        }
    }
}

#[async_trait]
pub trait WikiLlm: Send + Sync {
    /// One non-streaming structured-output call. Routes per (tenant, provider) inside S1.
    async fn create_message(&self, t: TenantId, req: WikiReq) -> Result<WikiResp, WikiLlmError>;
    /// Like [`create_message`](Self::create_message) but with a per-request turn observer threaded into
    /// the turn loop, so the model's REAL tool_call/tool_result steps stream out (the recall SSE driver
    /// and the WSS turn channel use this). Default: ignore the observer and delegate — extraction/Dream/
    /// stub paths don't stream. Only `RouterWikiLlm` overrides it to wire the sink into `TurnContext`.
    async fn create_message_observed(
        &self,
        t: TenantId,
        req: WikiReq,
        _sink: Option<std::sync::Arc<dyn protocol::TurnEventSink>>,
    ) -> Result<WikiResp, WikiLlmError> {
        self.create_message(t, req).await
    }
    /// Like [`create_message_observed`](Self::create_message_observed) but with a per-request route +
    /// reasoning-effort override (v0.2.2 recall model/effort picker). Default: ignore the override and
    /// delegate (stub/extraction/Dream don't honor it). Only `RouterWikiLlm` overrides this to root the
    /// per-tenant harness at the chosen (provider, model) and set the reasoning effort.
    async fn create_message_observed_with_override(
        &self,
        t: TenantId,
        req: WikiReq,
        sink: Option<std::sync::Arc<dyn protocol::TurnEventSink>>,
        _over: RecallOverride,
    ) -> Result<WikiResp, WikiLlmError> {
        self.create_message_observed(t, req, sink).await
    }
    /// Streaming synthesis (recall/query answers). Yields StreamEvent. (Recall is the next milestone;
    /// ingest never streams — the default is an empty stream so impls need only override when needed.)
    async fn create_message_stream(
        &self,
        _t: TenantId,
        _req: WikiReq,
    ) -> Result<StreamEventBox, WikiLlmError> {
        Ok(Box::pin(futures_util::stream::empty()))
    }
}

/// A deterministic, keyless, networkless `WikiLlm`. Scripts a queue of response bodies; an `__ERROR__`
/// body becomes a `WikiLlmError::Provider` (stage-4 failure-isolation tests). Records the last system
/// prefix + the last user-tail so tests can assert the cache-safe prefix / fenced-tail discipline.
pub struct StubWikiLlm {
    responses: Mutex<VecDeque<String>>,
    last_system: Mutex<String>,
    last_tail: Mutex<String>,
    calls: std::sync::atomic::AtomicU64,
}

impl StubWikiLlm {
    pub fn scripted(rs: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(rs.into()),
            last_system: Mutex::new(String::new()),
            last_tail: Mutex::new(String::new()),
            calls: std::sync::atomic::AtomicU64::new(0),
        }
    }
    /// A recorder that always returns a minimal well-formed SourceAnalysis (for prompt-shape asserts).
    pub fn recording() -> Self {
        Self::scripted(vec![
            r#"{"source_title":"x","summary":"y","entities":[],"concepts":[],"contradictions":[],"related_pages":[],"key_points":[]}"#
                .into(),
        ])
    }
    /// Counts calls but never has a script (every call errors-empty) — used to assert ZERO calls happen
    /// once the cost ceiling is hit before dispatch.
    pub fn counting() -> Self {
        Self::scripted(vec![])
    }
    pub fn last_system(&self) -> String {
        self.last_system.lock().map(|g| g.clone()).unwrap_or_default()
    }
    pub fn last_tail(&self) -> String {
        self.last_tail.lock().map(|g| g.clone()).unwrap_or_default()
    }
    pub fn call_count(&self) -> u64 {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl WikiLlm for StubWikiLlm {
    async fn create_message(&self, _t: TenantId, req: WikiReq) -> Result<WikiResp, WikiLlmError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut g) = self.last_system.lock() {
            *g = req.system.stable_prefix.clone();
        }
        let tail = req
            .messages
            .iter()
            .rev()
            .find_map(|m| m.content.clone())
            .unwrap_or_default();
        if let Ok(mut g) = self.last_tail.lock() {
            *g = tail;
        }
        let body = self
            .responses
            .lock()
            .ok()
            .and_then(|mut q| q.pop_front())
            .unwrap_or_else(|| "{}".into());
        if body == "__ERROR__" {
            return Err(WikiLlmError::Provider("scripted error".into()));
        }
        Ok(WikiResp { content: body, usage: None, truncated: false })
    }
}
