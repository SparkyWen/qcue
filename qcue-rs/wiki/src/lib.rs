//! QCue S2 — the Karpathy LLM-wiki data layer (LLM-free foundation).
//!
//! This crate owns the page model + frontmatter rules, the single central write-gate
//! (`write_gate::WikiWriteGate::write_page`) that is the ONLY body-write site, the pure-SQL lint
//! scanners over the Postgres link-graph, the per-tenant path-isolation guard, and the
//! `UNIVERSAL_LINK_CONSTRAINTS` prompt constant. Conversation-ingest, recall, query, and Auto-Dream
//! (the LLM-using parts) are deliberately NOT built here — clean seams are left for the next
//! milestone. Postgres is the query/lint substrate; the markdown body is the content source-of-truth
//! (dual representation, pitfall #12).

pub mod approvals;
pub mod clean_markdown;
pub mod conflict;
pub mod cost;
pub mod dream;
pub mod extract;
pub mod file_back;
pub mod index_gen;
pub mod ingest;
pub mod json_hardening;
pub mod lint;
pub mod llm;
pub mod page;
pub mod page_factory;
pub mod path_guard;
pub mod prompts;
pub mod query;
pub mod related;
pub mod sandbox;
pub mod types;
pub mod write_gate;

pub use page::{PageType, WikiPage};
