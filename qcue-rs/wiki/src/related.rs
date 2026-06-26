// QCue S2-R16 — related-pages matching is an in-memory slug/alias lookup over the tenant set (REPLACE
// the plugin's full vault scan). Bounded by the tenant's page list; never stuffs a page catalog into a
// prompt.
use crate::conflict::ExistingPage;
use slugify::slugify;

pub fn match_related<'a>(candidates: &[String], existing: &'a [ExistingPage]) -> Vec<&'a ExistingPage> {
    candidates
        .iter()
        .filter_map(|c| {
            let s = slugify(c);
            existing.iter().find(|p| p.slug == s || p.aliases.iter().any(|a| slugify(a) == s))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::conflict::ExistingPage;
    use uuid::Uuid;
    fn p(slug: &str, aliases: &[&str]) -> ExistingPage {
        ExistingPage {
            id: Uuid::now_v7(),
            slug: slug.into(),
            title: slug.into(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            r#type: "entity".into(),
        }
    }
    #[test]
    fn matches_in_memory_over_bounded_set_not_prompt_stuffed() {
        let pages = vec![p("rust", &["rustlang"]), p("tokio", &[])];
        // S2-R16 — slug/alias match over the bounded tenant set, never stuffing a page list into a prompt.
        let hits = match_related(&["rustlang".into(), "haskell".into()], &pages);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].slug, "rust");
    }
}
