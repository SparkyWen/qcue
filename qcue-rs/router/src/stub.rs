// QCue S1-R83, S1-R84 — keyless/networkless deterministic provider + scripting.
use protocol::{
    ApiError, Block, CanonicalUsage, Delta, FinishReason, NormalizedResponse, StreamEvent,
    StreamEventBox, ToolCall,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A scripted output. Built with combinators so §4–§12 invariants are testable without a network.
#[derive(Clone, Debug, Default)]
pub struct StubScript {
    pub thinking: Option<String>,
    pub text: Option<String>,
    pub tool: Option<(String, String)>, // (name, arguments-json-string)
    pub finish: FinishReasonScript,
    pub error: Option<ApiError>,
    pub reasoning_content: Option<String>, // for DeepSeek-quirk tests
    pub usage: Option<CanonicalUsage>,
}

#[derive(Clone, Copy, Debug)]
pub struct FinishReasonScript(pub FinishReason);
impl Default for FinishReasonScript {
    fn default() -> Self {
        Self(FinishReason::Stop)
    }
}

impl StubScript {
    pub fn text(t: &str) -> Self {
        Self { text: Some(t.into()), ..Default::default() }
    }
    pub fn thinking(t: &str) -> Self {
        Self { thinking: Some(t.into()), ..Default::default() }
    }
    pub fn tool_call(name: &str, args: &str) -> Self {
        Self {
            tool: Some((name.into(), args.into())),
            finish: FinishReasonScript(FinishReason::ToolCalls),
            ..Default::default()
        }
    }
    pub fn finish(fr: FinishReason) -> Self {
        Self { finish: FinishReasonScript(fr), ..Default::default() }
    }
    pub fn error(e: ApiError) -> Self {
        Self { error: Some(e), ..Default::default() }
    }
    pub fn with_text(mut self, t: &str) -> Self {
        self.text = Some(t.into());
        self
    }
    pub fn then_text(mut self, t: &str) -> Self {
        self.text = Some(t.into());
        self
    }
    pub fn with_reasoning_content(mut self, r: &str) -> Self {
        self.reasoning_content = Some(r.into());
        self
    }
    pub fn with_usage(mut self, u: CanonicalUsage) -> Self {
        self.usage = Some(u);
        self
    }
}

pub struct StubProvider {
    script: StubScript,
    network_calls: Arc<AtomicU64>,
    credential_reads: Arc<AtomicU64>,
}

impl StubProvider {
    pub fn new(script: StubScript) -> Self {
        Self {
            script,
            network_calls: Arc::new(AtomicU64::new(0)),
            credential_reads: Arc::new(AtomicU64::new(0)),
        }
    }
    pub fn network_calls(&self) -> u64 {
        self.network_calls.load(Ordering::SeqCst)
    }
    pub fn credential_reads(&self) -> u64 {
        self.credential_reads.load(Ordering::SeqCst)
    }

    /// A second handle that shares the same provider-invocation/credential-read counters, so a test
    /// can hold a counter while the harness takes ownership of the stub (turn-loop cost-cap test).
    pub fn clone_counter(&self) -> StubProvider {
        StubProvider {
            script: self.script.clone(),
            network_calls: self.network_calls.clone(),
            credential_reads: self.credential_reads.clone(),
        }
    }

    /// Non-stream completion (deterministic; never errors unless scripted).
    pub async fn complete(&self) -> Result<NormalizedResponse, ApiError> {
        // counts provider invocations: 0 only when the loop never reaches a call (e.g. cost-cap deny).
        self.network_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(e) = &self.script.error {
            return Err(e.clone());
        }
        let tool_calls = self.script.tool.as_ref().map(|(n, a)| {
            vec![ToolCall {
                id: Some("call_0".into()),
                name: n.clone(),
                arguments: a.clone(),
                provider_data: None,
            }]
        });
        Ok(NormalizedResponse {
            content: self.script.text.clone(),
            tool_calls,
            finish_reason: self.script.finish.0,
            reasoning: self.script.thinking.clone(),
            usage: self.script.usage,
            provider_data: self
                .script
                .reasoning_content
                .as_ref()
                .map(|r| serde_json::json!({ "reasoning_content": r })),
        })
    }

    /// Streamed output bracketed by MessageStart … MessageStop (S1-R20 taxonomy).
    pub fn stream(&self) -> StreamEventBox {
        // counts provider invocations (a driven stream legitimately counts one).
        self.network_calls.fetch_add(1, Ordering::SeqCst);
        let script = self.script.clone();
        Box::pin(async_stream::stream! {
            if let Some(e) = script.error { yield Err(e); return; }
            yield Ok(StreamEvent::MessageStart);
            if let Some(t) = script.thinking {
                yield Ok(StreamEvent::ContentBlockStart(Block::Thinking));
                yield Ok(StreamEvent::ContentBlockDelta(Delta::ThinkingDelta(t)));
                yield Ok(StreamEvent::ContentBlockStop);
            }
            if let Some(t) = script.text {
                yield Ok(StreamEvent::ContentBlockStart(Block::Text));
                yield Ok(StreamEvent::ContentBlockDelta(Delta::TextDelta(t)));
                yield Ok(StreamEvent::ContentBlockStop);
            }
            if let Some((name, args)) = script.tool {
                yield Ok(StreamEvent::ContentBlockStart(Block::ToolUse { id: "call_0".into(), name }));
                yield Ok(StreamEvent::ContentBlockDelta(Delta::InputJsonDelta { partial_json: args }));
                yield Ok(StreamEvent::ContentBlockStop);
            }
            yield Ok(StreamEvent::MessageDelta { stop_reason: Some(script.finish.0), usage: script.usage });
            yield Ok(StreamEvent::MessageStop);
        })
    }
}
