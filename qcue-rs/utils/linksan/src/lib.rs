// QCue S2-R49 (pure half) / pitfall #11 — link-pollution regexes + [[wikilink]] parser (ported from
// utils.ts / constraints.ts pollution-defense). The gate that *calls* sanitize_links lives in `wiki`;
// these are the pure regex transforms + the parser that mirrors links into the PG link-graph.
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLink {
    pub target_slug: String,         // bare slug, folder-prefix stripped
    pub target_type: Option<String>, // 'entity'|'concept'|'source' inferred from folder, else None
    pub display: Option<String>,     // [[slug|Display]]
}

// [[folder/slug|folder/slug]] → self-duplicated display (the #1 pollution shape).
#[allow(clippy::expect_used)]
static SELF_DUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\[(?:entities|concepts|sources)/([^|\]]+)\|(?:entities|concepts|sources)/[^\]]+\]\]")
        .expect("SELF_DUP regex is a valid literal")
});
// [[folder/folder/slug]] double-nested folder prefix.
#[allow(clippy::expect_used)]
static DOUBLE_NEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\[(?:entities|concepts|sources)/(?:entities|concepts|sources)/([^|\]]+)\]\]")
        .expect("DOUBLE_NEST regex is a valid literal")
});
// [[folder/<anything>|Display]] → collapse to the bare [[Display]] (drop the polluted folder slug).
// Only matches a folder-prefixed left side, so clean [[slug|Display]] is left intact.
#[allow(clippy::expect_used)]
static FOLDER_DISP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\[(?:entities|concepts|sources)/[^|\]]+\|([^\]]+)\]\]")
        .expect("FOLDER_DISP regex is a valid literal")
});
// Any [[wikilink]] body, for parsing the link-graph.
#[allow(clippy::expect_used)]
static ANY_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]]+)\]\]").expect("ANY_LINK regex is a valid literal"));

fn folder_to_type(folder: &str) -> Option<String> {
    match folder {
        "entities" => Some("entity".into()),
        "concepts" => Some("concept".into()),
        "sources" => Some("source".into()),
        _ => None,
    }
}

/// The central link sanitizer (pitfall #11): strip folder-prefix self-duplication / double-nesting and
/// collapse polluted `[[folder/...|Display]]` to a bare `[[Display]]`. Clean links pass through untouched.
pub fn sanitize_links(body: &str) -> String {
    let s = SELF_DUP.replace_all(body, "[[$1]]");
    let s = DOUBLE_NEST.replace_all(&s, "[[$1]]");
    let s = FOLDER_DISP.replace_all(&s, "[[$1]]");
    s.into_owned()
}

/// Parse all [[wikilinks]] for the link-graph upsert. Folder prefix → target_type; `slug|Display` split.
pub fn parse_wikilinks(body: &str) -> Vec<ParsedLink> {
    ANY_LINK
        .captures_iter(body)
        .map(|c| {
            let inner = &c[1];
            let (target, display) = match inner.split_once('|') {
                Some((t, d)) => (t.trim(), Some(d.trim().to_string())),
                None => (inner.trim(), None),
            };
            let (target_type, slug) = match target.split_once('/') {
                Some((folder, rest)) => (folder_to_type(folder), rest.to_string()),
                None => (None, target.to_string()),
            };
            ParsedLink { target_slug: slug, target_type, display }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    #[test]
    fn sanitize_strips_folder_prefix_self_duplication() {
        // [[entities/X|entities/X]] -> [[X]] ; [[concepts/conceptsFoo|Foo]] -> [[Foo]]
        assert_eq!(
            sanitize_links("see [[entities/X|entities/X]] and [[concepts/conceptsFoo|Foo]]"),
            "see [[X]] and [[Foo]]"
        );
        assert_eq!(sanitize_links("[[entities/entities/Y]]"), "[[Y]]"); // double-nested
        assert_eq!(sanitize_links("[[Plain]] [[slug|Disp]]"), "[[Plain]] [[slug|Disp]]"); // clean untouched
    }
    #[test]
    fn parse_extracts_links_with_type_and_display() {
        let links = parse_wikilinks("a [[entities/foo|Foo Display]] b [[bar]] c");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target_slug, "foo");
        assert_eq!(links[0].target_type.as_deref(), Some("entity"));
        assert_eq!(links[0].display.as_deref(), Some("Foo Display"));
        assert_eq!(links[1].target_slug, "bar");
        assert_eq!(links[1].target_type, None);
    }
}
