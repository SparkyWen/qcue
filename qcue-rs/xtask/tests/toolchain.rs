#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R5 — toolchain + edition pin asserted in CI
use std::fs;

#[test]
fn test_edition_toolchain_pinned() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/.."); // qcue-rs/
    let tc = fs::read_to_string(format!("{root}/rust-toolchain.toml")).unwrap();
    assert!(tc.contains("channel = \"1.88") || tc.contains("channel = \"1.9") || tc.contains("channel = \"1.8"),
            "toolchain must pin >=1.88, got:\n{tc}");
    let ws = fs::read_to_string(format!("{root}/Cargo.toml")).unwrap();
    assert!(ws.contains("resolver = \"2\""), "workspace must use resolver 2");
    assert!(ws.contains("edition = \"2024\""), "workspace.package must set edition 2024");
}

#[test]
fn test_clippy_lockdown_present() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let ws = fs::read_to_string(format!("{root}/Cargo.toml")).unwrap();
    for lint in ["unwrap_used", "expect_used", "redundant_clone"] {
        assert!(ws.contains(lint), "workspace.lints must deny {lint}");
    }
    assert!(ws.contains("\"deny\""), "lints must be set to deny");
}
