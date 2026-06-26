// QCue S2-R8/S2-R12 — extraction + report DTOs (types.ts:5-16,38-43,185-200). deny_unknown_fields +
// default. These are pure serde data definitions; the LLM extraction/ingest pipeline that *produces*
// them is built in the next milestone — these types are the clean seam it will fill.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ItemInfo {
    pub name: String,
    pub aliases: Vec<String>,
    pub subtype: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContradictionInfo {
    pub claim: String,
    pub source_page: Option<String>,
    pub contradicted_by: Option<String>,
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SourceAnalysis {
    pub source_title: String,
    pub summary: String,
    pub entities: Vec<ItemInfo>,
    pub concepts: Vec<ItemInfo>,
    pub contradictions: Vec<ContradictionInfo>,
    pub related_pages: Vec<String>,
    pub key_points: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IngestReport {
    pub created_pages: Vec<Uuid>,
    pub merged_pages: Vec<Uuid>,
    pub skipped_redundant: bool,
    pub contradictions: Vec<Uuid>,
    pub errors: Vec<String>,
}

/// A structured "missing concept page" row (S2-R45) — not prose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingPage {
    pub name: String,
    pub source: String,
    pub reason: String,
}

/// Result of ConflictResolver::resolve (S2-R20).
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveAction {
    Create,
    Merge,
    Flag,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConflictResolution {
    pub action: ResolveAction,
    pub target: Option<Uuid>,
    pub existing_type: Option<String>,
    pub confidence: f32,
    pub reason: String,
}

/// The NO_NEW_CONTENT sentinel returned by the LLM body-merge (S2-R22).
pub const NO_NEW_CONTENT: &str = "NO_NEW_CONTENT";

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use serde_json::json;
    #[test]
    fn source_analysis_deserializes_and_rejects_unknown_fields() {
        let v = json!({
            "source_title": "Notes on Rust",
            "summary": "A short summary.",
            "entities": [{"name":"Rust","aliases":["rustlang"],"subtype":"product"}],
            "concepts": [{"name":"Ownership","aliases":[],"subtype":"method"}],
            "contradictions": [],
            "related_pages": ["tokio"],
            "key_points": ["memory safety"]
        });
        let sa: SourceAnalysis = serde_json::from_value(v).unwrap();
        assert_eq!(sa.source_title, "Notes on Rust");
        assert_eq!(sa.entities.len(), 1);
        // unknown field rejected (B-R8)
        let bad = json!({"source_title":"x","summary":"y","surprise":true});
        assert!(serde_json::from_value::<SourceAnalysis>(bad).is_err());
    }
    #[test]
    fn ingest_report_has_skipped_redundant_and_errors() {
        let r = IngestReport::default();
        assert!(!r.skipped_redundant);
        assert!(r.errors.is_empty());
    }
}
