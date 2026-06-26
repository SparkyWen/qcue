// QCue S2-R1 — architecture test: the wiki data layer must not depend on providers/http (it is
// provider-agnostic; the LLM seam is a trait the next milestone fills, never a transport import).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::fs;

#[test]
fn wiki_has_no_provider_or_http_dep() {
    let toml = fs::read_to_string("Cargo.toml").unwrap();
    assert!(!toml.contains("providers ="), "wiki must not depend on providers");
    assert!(!toml.contains("http ="), "wiki must not depend on http");
    assert!(!toml.contains("reqwest"), "wiki must not depend on reqwest directly");
    // The LLM seam is a trait the upper layer fills (RouterWikiLlm lives in app-server); the wiki crate
    // must never import the router/transport directly (S2-R1 — provider-agnostic).
    assert!(!toml.contains("router ="), "wiki must not depend on router (the seam is a trait)");
}
