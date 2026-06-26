// QCue S2 — programmatic lint. The scanners are pure SQL over the PG link-graph (no LLM, no markdown
// reads, B-R16/pitfall #12). LLM-assisted duplicate-verification and the destructive Smart-Fix-All
// (reversible via soft-delete + approvals) are the next milestone; the scanners here are read-only.
pub mod scanners;
