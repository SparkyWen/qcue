// QCue S2-R20 — pure deterministic create/merge/flag ladder (PORT conflict-resolver.ts:75-125). No IO, no LLM.
use crate::types::{ConflictResolution, ResolveAction};
use slugify::slugify;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ExistingPage {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub r#type: String,
}

pub struct ConflictResolver;
impl ConflictResolver {
    pub fn resolve(name: &str, want_type: &str, existing: &[ExistingPage]) -> ConflictResolution {
        let slug = slugify(name);
        let name_l = name.to_lowercase();
        // 1) same-type exact slug
        if let Some(p) = existing.iter().find(|p| p.r#type == want_type && p.slug == slug) {
            return merge(p, "same-type exact slug");
        }
        // 2) same-type slug/alias (alias-aware)
        if let Some(p) = existing.iter().find(|p| {
            p.r#type == want_type
                && (p.slug == slug
                    || p.aliases.iter().any(|a| a.to_lowercase() == name_l || slugify(a) == slug))
        }) {
            return merge(p, "same-type alias match");
        }
        // 3) cross-type slug/alias → flag (page-factory bridges via the existing type)
        if let Some(p) =
            existing.iter().find(|p| p.slug == slug || p.aliases.iter().any(|a| slugify(a) == slug))
        {
            return ConflictResolution {
                action: ResolveAction::Flag,
                target: Some(p.id),
                existing_type: Some(p.r#type.clone()),
                confidence: 0.9,
                reason: "cross-type slug/alias collision".into(),
            };
        }
        // 4) else create (the LLM semantic-dedup runs ONLY when this returns Create — caller decides)
        ConflictResolution {
            action: ResolveAction::Create,
            target: None,
            existing_type: None,
            confidence: 1.0,
            reason: "no deterministic match".into(),
        }
    }
}

fn merge(p: &ExistingPage, reason: &str) -> ConflictResolution {
    ConflictResolution {
        action: ResolveAction::Merge,
        target: Some(p.id),
        existing_type: Some(p.r#type.clone()),
        confidence: 1.0,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::types::ResolveAction;
    use uuid::Uuid;
    fn page(slug: &str, aliases: &[&str], ty: &str) -> ExistingPage {
        ExistingPage {
            id: Uuid::nil(),
            slug: slug.into(),
            title: slug.into(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            r#type: ty.into(),
        }
    }
    #[test]
    fn deterministic_ladder_resolves_without_llm() {
        let existing = vec![page("rust", &["rustlang"], "entity")];
        // same-type exact slug → Merge target
        let r = ConflictResolver::resolve("Rust", "entity", &existing);
        assert_eq!(r.action, ResolveAction::Merge);
        assert!(r.confidence >= 0.99);
        // same-type alias hit → Merge
        let r = ConflictResolver::resolve("rustlang", "entity", &existing);
        assert_eq!(r.action, ResolveAction::Merge);
        // cross-type slug hit → Flag (cross-type collision; page-factory bridges)
        let r = ConflictResolver::resolve("Rust", "concept", &existing);
        assert_eq!(r.action, ResolveAction::Flag);
        // no match → Create
        let r = ConflictResolver::resolve("Haskell", "entity", &existing);
        assert_eq!(r.action, ResolveAction::Create);
    }
}
