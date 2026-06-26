// QCue S2-R34 / A-R35 — the MVP migrations contain NO pgvector extension / vector column (D14,
// pitfall #13). Index-first retrieval is the whole point; embeddings are an M6+ complementary channel
// behind a per-tenant capacity trigger, never the MVP path. This is the shared assertion that keeps a
// stray `CREATE EXTENSION vector` / `vector(...)` column out of M0..M5.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::fs;

#[test]
fn no_pgvector_in_mvp_migrations() {
    // migrations live at the workspace root `qcue-rs/migrations`; tests run from the `wiki` crate dir.
    let dir = "../migrations";
    let mut checked = 0usize;
    for entry in fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        // pgvector is the M6+ slot only; an M6_* migration may legitimately create the extension.
        if name.contains("_M6_") || name.starts_with("M6_") {
            continue;
        }
        let sql = fs::read_to_string(&p).unwrap_or_default().to_lowercase();
        assert!(
            !sql.contains("extension vector") && !sql.contains("extension \"vector\""),
            "{name} creates the vector extension"
        );
        assert!(!sql.contains(" vector("), "{name} declares a vector column");
        checked += 1;
    }
    assert!(checked > 0, "no migrations were scanned — wrong path?");
}
