//! QCue S2 — the three recall systems (the agentic-recall blueprint, Appendix A).
//!
//! The CORE PRINCIPLE: recall is NOT a fixed retrieval step. The harness gives the model a search tool
//! (`recall_search`) and the MODEL authors its own search pattern; the harness only routes that pattern
//! to an index path (`route`) and executes it (`search_tool` over `store::SearchRepo`). Three systems:
//!   1. agentic capture/transcript SEARCH (`search_tool` + `route`),
//!   2. curated memory frozen into the stable prefix (`curated`),
//!   3. passive sideQuery prefetch fenced into the message TAIL (`prefetch`).
//!
//! `tool_policy` is the one read-only sandbox both recall and (the next milestone's) Dream share.
pub mod curated;
pub mod prefetch;
pub mod prompt;
pub mod route;
pub mod search_tool;
pub mod tool_policy;
