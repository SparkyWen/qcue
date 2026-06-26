//! QCue S3-R71 / B-R29 — every filesystem write derives from QCUE_DATA_ROOT, realpath-guarded,
//! append-only. The canonical raw capture is the JSONL line written here BEFORE any LLM call (the
//! Postgres `ideas` row is a derived index, §9). The line is redacted (B-R11) so no secret lands on disk.
use crate::redact::redact_json;
use crate::wire::path_guard::resolve_under_root_ext;
use std::io::Write;
use std::path::PathBuf;
use uuid::Uuid;

pub struct ObjStore {
    root: PathBuf, // = <QCUE_DATA_ROOT>/objects
}

impl ObjStore {
    pub fn new(data_root: &str) -> Self {
        ObjStore { root: PathBuf::from(data_root).join("objects") }
    }

    /// Append a canonical JSONL capture line BEFORE any LLM call (B-R27/B-R29). Returns the log_ref key.
    pub fn append_capture(
        &self,
        tenant: Uuid,
        user: Uuid,
        idea_id: Uuid,
        payload: &serde_json::Value,
    ) -> std::io::Result<String> {
        let rel = format!("t/{tenant}/u/{user}/captures/{idea_id}.jsonl");
        let base = self.root.join(format!("t/{tenant}/u/{user}"));
        let full = self.root.join(&rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // ensure the write stays under the per-user base even after create (realpath + prefix check).
        resolve_under_root_ext(&base, &format!("captures/{idea_id}.jsonl"), &["jsonl"])
            .map_err(|_| std::io::Error::other("path guard rejected capture key"))?;
        let mut line = serde_json::json!({
            "ts": chrono::Utc::now(), "schema_version": 1, "kind": "capture", "payload": payload
        });
        redact_json(&mut line); // B-R11: never let a secret reach the object store
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&full)?;
        writeln!(f, "{}", serde_json::to_string(&line).unwrap_or_default())?;
        Ok(rel)
    }
}
