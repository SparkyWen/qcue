// QCue S2 / App. A §2.4 — the harness-driven Auto-Dream agent. The lock-as-clock, the gate ladder, and
// the candidates→confirm gate are PURE/PG and live in `wiki`; the `DreamAgent` (the run_turn fork that
// drives the LLM through the recall tool policy) lives HERE, where the shared `build_tool_policy` + the
// router seam are reachable. `DreamAgent` implements `wiki::dream::scheduler::DreamRunner`, so the
// wiki-side scheduler drives it without `wiki` ever reaching the router (provider-agnostic).
pub mod agent;
