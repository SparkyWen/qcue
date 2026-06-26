#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R1 / S1-R2 — build-graph layering law + protocol minimal deps
use xtask::lints::{check_layering_law, check_protocol_deps_minimal, workspace_root};

#[test]
fn test_layering_law() {
    // protocol ← http ← llm-api ← providers ← router; lower never imports upper.
    let violations = check_layering_law(&workspace_root());
    assert!(violations.is_empty(), "upward dependency edges found: {violations:?}");
}

#[test]
fn test_protocol_deps_minimal() {
    // protocol's [dependencies] ⊆ allowlist; no async/tokio tokens in protocol/src.
    let problems = check_protocol_deps_minimal(&workspace_root());
    assert!(problems.is_empty(), "protocol purity violations: {problems:?}");
}
