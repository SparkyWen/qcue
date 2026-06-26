// QCue S2-R6 / B-R30 — YAML frontmatter split/merge (created/updated/reviewed are system fields, B-R7).
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Frontmatter {
    pub r#type: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub sources: Vec<String>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub reviewed: bool,
}

/// Split `---\n…\n---\nbody` into (Frontmatter, body). Missing frontmatter → defaults + whole input as body.
pub fn parse_frontmatter(md: &str) -> (Frontmatter, String) {
    if let Some(rest) = md.strip_prefix("---\n")
        && let Some(end) = rest.find("\n---")
    {
        let yaml = &rest[..end];
        let body = rest[end + 4..].trim_start_matches('\n').to_string();
        let fm = serde_yaml::from_str::<Frontmatter>(yaml).unwrap_or_default();
        return (fm, body);
    }
    (Frontmatter::default(), md.to_string())
}

/// Render frontmatter + body back to markdown (stable key order for cache-byte stability, pitfall #2).
pub fn render_frontmatter(fm: &Frontmatter, body: &str) -> String {
    let yaml = serde_yaml::to_string(fm).unwrap_or_default();
    format!("---\n{yaml}---\n{body}\n")
}

/// Programmatic merge (S2-R22): keep oldest `created`, bump `updated` to `now`, append+dedup sources/tags/aliases.
pub fn merge_frontmatter(into: &mut Frontmatter, from: &Frontmatter, now: &str) {
    if let (Some(a), Some(b)) = (&into.created, &from.created) {
        if b < a {
            into.created = Some(b.clone());
        }
    } else if into.created.is_none() {
        into.created = from.created.clone().or_else(|| Some(now.to_string()));
    }
    into.updated = Some(now.to_string());
    for s in &from.sources {
        if !into.sources.contains(s) {
            into.sources.push(s.clone());
        }
    }
    for t in &from.tags {
        if !into.tags.contains(t) {
            into.tags.push(t.clone());
        }
    }
    for a in &from.aliases {
        if !into.aliases.contains(a) {
            into.aliases.push(a.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    const MD: &str = "---\ntype: entity\ncreated: 2026-01-01T00:00:00Z\nupdated: 2026-01-02T00:00:00Z\nsources:\n  - idea-1\ntags:\n  - person\naliases:\n  - DSA\nreviewed: true\n---\nBody text here.\n";
    #[test]
    fn parse_splits_frontmatter_and_body() {
        let (fm, body) = parse_frontmatter(MD);
        assert_eq!(fm.r#type.as_deref(), Some("entity"));
        assert_eq!(fm.aliases, vec!["DSA".to_string()]);
        assert!(fm.reviewed);
        assert_eq!(body.trim(), "Body text here.");
    }
    #[test]
    fn merge_keeps_oldest_created_bumps_updated_appends_sources() {
        let mut a = Frontmatter {
            created: Some("2026-01-01T00:00:00Z".into()),
            updated: Some("2026-01-01T00:00:00Z".into()),
            sources: vec!["idea-1".into()],
            ..Default::default()
        };
        let b = Frontmatter {
            created: Some("2026-05-01T00:00:00Z".into()),
            updated: Some("2026-05-01T00:00:00Z".into()),
            sources: vec!["idea-2".into()],
            ..Default::default()
        };
        merge_frontmatter(&mut a, &b, "2026-06-13T00:00:00Z");
        assert_eq!(a.created.as_deref(), Some("2026-01-01T00:00:00Z")); // oldest kept
        assert_eq!(a.updated.as_deref(), Some("2026-06-13T00:00:00Z")); // bumped to now
        assert_eq!(a.sources, vec!["idea-1".to_string(), "idea-2".to_string()]); // appended, deduped
    }
    #[test]
    fn render_roundtrips() {
        let (fm, body) = parse_frontmatter(MD);
        let rendered = render_frontmatter(&fm, &body);
        let (fm2, _) = parse_frontmatter(&rendered);
        assert_eq!(fm2.aliases, fm.aliases);
        assert_eq!(fm2.reviewed, fm.reviewed);
    }
}
