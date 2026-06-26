//! QCue sync wire types (SYNC-D8): the op, the pull delta, and the snapshot bootstrap.
//! serde-only — no async/tokio/sqlx (protocol crate layering law).
//! Spec: docs/superpowers/specs/2026-06-15-multiplatform-sync-design.md
use serde::{Deserialize, Serialize};

/// One HLC-stamped op. `op` is the opaque CRDT bag (B-R8); the HLC tuple totally orders (B-R21/D6).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SyncOp {
    pub hlc_wall_ms: i64,
    pub hlc_lamport: i64,
    pub site_id: i64,
    /// "idea" | "wiki_page"
    pub entity_kind: String,
    /// client_uuid (idea) | slug (wiki_page)
    pub entity_ref: String,
    pub op: serde_json::Value,
}

/// A wiki page in the cold-start snapshot (body omitted — fetched by hash if the client lacks it).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WikiPageSnap {
    pub slug: String,
    pub title: String,
    pub content_hash: String,
    pub sync_version: i64,
}

/// An idea (capture) in the cold-start snapshot.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct IdeaSnap {
    pub id: String,
    pub body: String,
    pub origin: String,
    pub captured_at: String,
}

/// Cold-start snapshot of the canonical tables (SYNC-D5).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SyncSnapshot {
    pub ideas: Vec<IdeaSnap>,
    pub wiki_pages: Vec<WikiPageSnap>,
}

/// The pull response: either a snapshot (cold start) or incremental ops, plus the next cursor (seq).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SyncDelta {
    pub cursor: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub snapshot: Option<SyncSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ops: Vec<SyncOp>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    #[test]
    fn syncop_round_trips() {
        let op = SyncOp {
            hlc_wall_ms: 1,
            hlc_lamport: 2,
            site_id: 3,
            entity_kind: "wiki_page".into(),
            entity_ref: "foo".into(),
            op: serde_json::json!({"set_body": "x", "base_version": 0}),
        };
        let s = serde_json::to_string(&op).unwrap();
        assert_eq!(serde_json::from_str::<SyncOp>(&s).unwrap().entity_ref, "foo");
    }

    #[test]
    fn delta_snapshot_and_incremental_shapes() {
        let snap = SyncDelta {
            cursor: 9,
            snapshot: Some(SyncSnapshot {
                ideas: vec![IdeaSnap {
                    id: "i1".into(),
                    body: "b".into(),
                    origin: "text".into(),
                    captured_at: "2026-06-15T00:00:00Z".into(),
                }],
                wiki_pages: vec![WikiPageSnap {
                    slug: "s".into(),
                    title: "t".into(),
                    content_hash: "h".into(),
                    sync_version: 1,
                }],
            }),
            ops: vec![],
        };
        let round = serde_json::from_str::<SyncDelta>(&serde_json::to_string(&snap).unwrap()).unwrap();
        assert_eq!(round, snap);
        // incremental: no snapshot key serialized
        let incr = SyncDelta { cursor: 10, snapshot: None, ops: vec![] };
        let j = serde_json::to_string(&incr).unwrap();
        assert!(!j.contains("snapshot"));
    }
}
