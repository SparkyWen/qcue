// QCue S2-R49 — there is exactly ONE body-write call site (pitfall #11). Scan the crate for the async
// page-body write (`tokio::fs::write`); only `write_gate.rs` is allowed to perform it. (Sync `fs::write`
// is used only by test fixtures to seed bodies and is not a production page-body write path.)
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::fs;

#[test]
fn only_write_gate_writes_page_bodies() {
    let mut hits = vec![];
    for entry in walkdir::WalkDir::new("src").into_iter().filter_map(Result::ok) {
        if entry.path().extension().is_some_and(|e| e == "rs") {
            let src = fs::read_to_string(entry.path()).unwrap();
            if src.contains("tokio::fs::write(") {
                hits.push(entry.path().display().to_string());
            }
        }
    }
    // the ONLY production file allowed to write a page body is write_gate.rs
    hits.retain(|p| !p.ends_with("write_gate.rs"));
    assert!(hits.is_empty(), "page body writes outside the write-gate: {hits:?}");
}
