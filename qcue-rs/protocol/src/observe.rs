//! QCue — the turn-event sink (observer seam).
//!
//! Lets a higher crate watch a turn's progress — the assistant's content, each model-authored tool call,
//! each tool result — WITHOUT the turn loop knowing who is watching (the same decoupling the harness uses
//! everywhere; S1-R89: the loop never branches on the consumer).
//!
//! It lives in `protocol` on purpose: `router` (which owns `run_turn`/`TurnContext`) and `wiki` (which
//! owns `WikiReq`/`WikiLlm`, the only path the SSE recall driver has into `run_turn`) are independent
//! sibling crates — neither depends on the other — and `protocol` is the one crate BOTH depend on. The
//! trait is deliberately **synchronous** and push-only: `protocol` is serde-pure and the xtask
//! protocol-purity lint forbids the tokens `async fn`/`tokio::`, so the trait carries no async surface.
//! Concrete impls (the recall `StreamHubSink`, the WSS `WsTurnSink`) own the tokio channel / broadcast
//! hub and push **non-blocking** (`try_send` / bounded broadcast) — the sink runs INLINE on the turn
//! task, so a blocking impl would stall the loop.

/// A non-blocking observer of an in-flight turn. Every method defaults to a no-op, so attaching a sink is
/// byte-transparent to every existing turn (`None` ⇒ identical behavior). `iter` is the 1-based turn-loop
/// iteration the event came from, so a consumer can correlate calls and results within a turn.
pub trait TurnEventSink: Send + Sync {
    /// The assistant's text content produced on `iter` (the final answer, or interim prose alongside a
    /// tool call). Empty content is possible — impls decide whether to forward it.
    fn on_assistant_delta(&self, _iter: u32, _content: &str) {}

    /// A model-authored tool call: `name` + the verbatim JSON `arguments` (byte-stable, NEVER
    /// re-serialized — pitfall: `ToolCall.arguments` must stay byte-identical for prompt cache / A-R13).
    fn on_tool_call(&self, _iter: u32, _name: &str, _arguments: &str) {}

    /// The tool result fed back to the model on `iter` (`name` = the tool, `content` = the result text).
    fn on_tool_result(&self, _iter: u32, _name: &str, _content: &str) {}
}
