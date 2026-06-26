// QCue S2 — the wiki page model + PageType (mirrors the `wiki_page_type` PG enum, Appendix B §2.1).
//
// Dual representation: the markdown body is the content source-of-truth; the structured fields here
// (slug/title/aliases/tags/summary/char_len/dates/reviewed) mirror into Postgres for query/lint
// (pitfall #12 — lint never reads markdown bodies). `created`/`updated`/`char_len` are SYSTEM-set on
// write, never LLM-set (B-R7); a `reviewed:true` page is protected from auto-rewrite (wiki-engine §4).
use serde::{Deserialize, Serialize};

/// The wiki page type. `comparison`/`overview` are reserved for query-answers-filed-back (wiki-engine
/// §1). Matches the `wiki_page_type` enum declared in the §2.1 prelude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageType {
    Entity,
    Concept,
    Source,
    Index,
    Log,
    Contradiction,
    Schema,
    Comparison,
    Overview,
}

impl PageType {
    /// The PG enum label / frontmatter `type` string.
    pub fn as_str(self) -> &'static str {
        match self {
            PageType::Entity => "entity",
            PageType::Concept => "concept",
            PageType::Source => "source",
            PageType::Index => "index",
            PageType::Log => "log",
            PageType::Contradiction => "contradiction",
            PageType::Schema => "schema",
            PageType::Comparison => "comparison",
            PageType::Overview => "overview",
        }
    }

    /// The vault folder a page of this type lives in, if any. Only entity/concept/source pages live in
    /// a typed folder; structural pages (index/log/…) live at the vault root.
    pub fn folder(self) -> Option<&'static str> {
        match self {
            PageType::Entity => Some("entities"),
            PageType::Concept => Some("concepts"),
            PageType::Source => Some("sources"),
            _ => None,
        }
    }

    pub fn parse(s: &str) -> Option<PageType> {
        Some(match s {
            "entity" => PageType::Entity,
            "concept" => PageType::Concept,
            "source" => PageType::Source,
            "index" => PageType::Index,
            "log" => PageType::Log,
            "contradiction" => PageType::Contradiction,
            "schema" => PageType::Schema,
            "comparison" => PageType::Comparison,
            "overview" => PageType::Overview,
            _ => return None,
        })
    }
}

/// The structured mirror of a wiki page (the Postgres projection of frontmatter + body length). The
/// markdown body itself is not held here — it lives at `body_ref` in the vault.
#[derive(Debug, Clone, PartialEq)]
pub struct WikiPage {
    pub r#type: PageType,
    pub slug: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub summary: String,
    /// SYSTEM-set on write (the sanitized body char count); never LLM-set (B-R7, pitfall #12).
    pub char_len: i32,
    pub body_ref: String,
    pub source_ids: Vec<uuid::Uuid>,
    /// Human-verified → protected from auto-rewrite (wiki-engine §4). DB-controlled, not LLM-set.
    pub reviewed: bool,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    #[test]
    fn page_type_roundtrips_and_folders() {
        for t in [
            PageType::Entity,
            PageType::Concept,
            PageType::Source,
            PageType::Index,
            PageType::Log,
            PageType::Contradiction,
            PageType::Schema,
            PageType::Comparison,
            PageType::Overview,
        ] {
            assert_eq!(PageType::parse(t.as_str()), Some(t));
        }
        assert_eq!(PageType::Entity.folder(), Some("entities"));
        assert_eq!(PageType::Concept.folder(), Some("concepts"));
        assert_eq!(PageType::Source.folder(), Some("sources"));
        assert_eq!(PageType::Index.folder(), None); // structural pages live at the vault root
        assert_eq!(PageType::parse("nonsense"), None);
    }
}
