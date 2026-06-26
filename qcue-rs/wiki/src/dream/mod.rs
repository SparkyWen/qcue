// QCue S2 / App. A — the Auto-Dream slice. The PURE/PG pieces live here (provider-agnostic, no router
// reach): the lock-as-clock (`lock`), the cheapest-gate-first ladder + scheduler (`scheduler`), and the
// candidates→confirm approvals gate (`crate::approvals`). The harness-driven `DreamAgent` (which drives
// `router::run_turn` through the recall tool policy) lives in the `ideas` crate where the router seam +
// the shared `build_tool_policy` are reachable — `wiki` stays clean (LLM only via `WikiLlm`).
pub mod lock;
pub mod scheduler;
