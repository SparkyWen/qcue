// QCue S1-R48 (dispatch seam) + A-R12/A-R40 read-only ToolPolicy builder (S2 registers the tools).
use async_trait::async_trait;
use protocol::{Message, Role, ToolCall, ToolDef};
use std::sync::Arc;

/// The EXECUTION seam for model-authored read-only tools (`recall_search` / `read_page` / `read_lines`).
/// The real impl lives in a higher crate (app-server) that can reach the tenant's captures + wiki; it is
/// constructed already tenant-bound. Returns the tool-result content the model reads next; an `Err` is
/// surfaced AS the tool result (so the model can recover, not hard-fail the turn). When no handler is
/// registered the dispatcher falls back to a stub result, keeping the keyless turn tests byte-identical.
#[async_trait]
pub trait ToolExec: Send + Sync {
    async fn call(&self, name: &str, arguments: &str) -> Result<String, String>;
}

/// A read-only / realpath-confined policy. Recall = read-only; Dream adds propose_* flags.
/// S1 owns the BUILDER; S2's tool impls enforce realpath at execution. NOTE: this is a legacy seam —
/// the live recall web capability is NOT gated here but via `ideas::recall::tool_policy` (`allow_web`)
/// + the `RecallToolExec` web client (see docs/test/harness-eval.md); `allow_network` below stays false.
#[derive(Clone, Debug)]
pub struct ReadOnlyToolPolicy {
    pub allow_network: bool,        // legacy flag, left false; web is gated in the app-server executor seam
    pub allow_propose_writes: bool, // false for recall, true for Dream (root-confined)
    pub wall_clock_cap_ms: u64,     // default 120_000
    pub tool_call_cap: u32,         // default 30
}
impl ReadOnlyToolPolicy {
    /// The single builder; recall and Dream differ ONLY by the propose flag + prompt (A-R40).
    pub fn build(allow_propose_writes: bool) -> Self {
        Self {
            allow_network: false,
            allow_propose_writes,
            wall_clock_cap_ms: 120_000,
            tool_call_cap: 30,
        }
    }
    pub fn recall() -> Self {
        Self::build(false)
    }
    pub fn dream() -> Self {
        Self::build(true)
    }
}

/// The dispatch seam. Read-only tools (recall_search/read_page/read_lines). When an `exec` handler is
/// registered (`with_handler`) the model's tool calls run for real (against the tenant's captures/wiki);
/// otherwise a stub result is returned (keyless tests + plain extraction).
pub struct ToolDispatcher {
    #[allow(dead_code)]
    echo_name: Option<String>,
    /// The tool definitions advertised to the model on each call (empty when none registered).
    defs: Vec<ToolDef>,
    /// The real execution handler (tenant-bound), injected by S2/recall. `None` → stub results.
    exec: Option<Arc<dyn ToolExec>>,
}
impl ToolDispatcher {
    pub fn empty() -> Self {
        Self { echo_name: None, defs: Vec::new(), exec: None }
    }
    /// A test dispatcher that returns a fixed tool result for `name` (drives the iteration-cap test).
    pub fn echo(name: &str) -> Self {
        Self { echo_name: Some(name.into()), defs: Vec::new(), exec: None }
    }

    /// Construct a dispatcher that advertises `defs` to the model but stub-executes them (extraction,
    /// or a not-yet-wired tool seam).
    pub fn with_defs(defs: Vec<ToolDef>) -> Self {
        Self { echo_name: None, defs, exec: None }
    }

    /// Construct a dispatcher that advertises `defs` AND runs them for real via `exec` (recall: the
    /// model authors `recall_search` and the handler executes a real, RLS-scoped search).
    pub fn with_handler(defs: Vec<ToolDef>, exec: Arc<dyn ToolExec>) -> Self {
        Self { echo_name: None, defs, exec: Some(exec) }
    }

    /// The tool definitions sent to the model on each call (S1-R48 dispatch seam → DispatchRequest).
    pub fn defs(&self) -> Vec<ToolDef> {
        self.defs.clone()
    }

    /// Execute tool calls → role:tool result messages. With a registered `exec` the calls run for real;
    /// otherwise a stub result is returned. A handler error is surfaced AS the tool result so the model
    /// can recover (it never hard-fails the turn). Tool results are always marked untrusted.
    pub async fn dispatch(&self, calls: &[ToolCall]) -> Vec<Message> {
        let mut out = Vec::with_capacity(calls.len());
        for tc in calls {
            let content = match &self.exec {
                Some(exec) => match exec.call(&tc.name, &tc.arguments).await {
                    Ok(s) => s,
                    Err(e) => format!("tool error ({}): {e}", tc.name),
                },
                None => format!("result for {}", tc.name),
            };
            out.push(Message {
                role: Role::Tool,
                content: Some(content),
                tool_call_id: tc.id.clone(),
                tool_name: Some(tc.name.clone()),
                tool_calls: None,
                finish_reason: None,
                reasoning: None,
                provider_data: None,
                active: true,
                is_untrusted: true,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    struct SpyExec {
        seen: std::sync::Mutex<Vec<(String, String)>>,
    }
    #[async_trait]
    impl ToolExec for SpyExec {
        async fn call(&self, name: &str, arguments: &str) -> Result<String, String> {
            self.seen.lock().unwrap().push((name.into(), arguments.into()));
            Ok(format!("REAL[{name}]"))
        }
    }

    fn tc(name: &str, args: &str) -> ToolCall {
        ToolCall { id: Some("c0".into()), name: name.into(), arguments: args.into(), provider_data: None }
    }

    #[tokio::test]
    async fn dispatch_without_handler_returns_stub_result() {
        // The keyless path is unchanged: no handler → the old stub result (existing turn tests rely on this).
        let d = ToolDispatcher::with_defs(vec![]);
        let out = d.dispatch(&[tc("recall_search", "{}")]).await;
        assert_eq!(out[0].content.as_deref(), Some("result for recall_search"));
    }

    #[tokio::test]
    async fn dispatch_with_handler_routes_to_real_exec_with_model_pattern() {
        // The agentic path: the dispatcher routes the model's call (verbatim arguments) to the real handler.
        let spy = Arc::new(SpyExec { seen: std::sync::Mutex::new(vec![]) });
        let d = ToolDispatcher::with_handler(vec![], spy.clone());
        let out = d.dispatch(&[tc("recall_search", r#"{"pattern":"postgres migration"}"#)]).await;
        assert_eq!(out[0].content.as_deref(), Some("REAL[recall_search]"));
        assert!(out[0].is_untrusted, "tool results are untrusted");
        let seen = spy.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "recall_search");
        assert!(seen[0].1.contains("postgres migration"), "model's pattern reaches the handler verbatim (A-R13)");
    }

    #[tokio::test]
    async fn dispatch_handler_error_is_surfaced_as_tool_result() {
        struct ErrExec;
        #[async_trait]
        impl ToolExec for ErrExec {
            async fn call(&self, _n: &str, _a: &str) -> Result<String, String> {
                Err("boom".into())
            }
        }
        let d = ToolDispatcher::with_handler(vec![], Arc::new(ErrExec));
        let out = d.dispatch(&[tc("read_page", "{}")]).await;
        assert!(out[0].content.as_deref().unwrap().contains("tool error"), "errors recover, not hard-fail");
    }
}
