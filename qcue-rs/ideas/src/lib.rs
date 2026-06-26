//! QCue S2 — the `ideas` crate: capture entry (persist-first) + the untrusted-content fence + the three
//! recall systems (the agentic-recall blueprint). Recall is NOT a fixed retrieval step: the harness
//! gives the model a `recall_search` tool and the MODEL authors its own search pattern (`recall`).
//!
//! Layering: `ideas` is provider-agnostic (no `providers`/`http`/`reqwest`); the LLM is reached only
//! through the `wiki::llm::WikiLlm` seam by the ingest/query job that an upper layer (app-server) drives.
pub mod capture;
pub mod dream;
pub mod fence;
pub mod recall;
