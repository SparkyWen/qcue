// QCue B-R24/B-R25 — CJK search routing (Hermes FTS5/trigram → Postgres). Pure, unit-tested w/o PG.
//
// A leaf micro-crate so BOTH `store` (the `SearchRepo` executor that branches the SQL per mode) and
// `ideas` (the `recall_search` tool that routes the model's pattern) can share `SearchMode` +
// `route_search` without either importing the other — preserving the sibling layering law (the same
// refactor pattern as `utils/fence`). The CORE PRINCIPLE: the model authors the pattern; this function
// only picks the index path (tsvector / pg_trgm / ILIKE), never rewrites the pattern (A-R13).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// Postgres FTS — a real (non-CJK) token of ≥3 chars.
    Tsvector,
    /// pg_trgm similarity — a CJK run of ≥2 chars (substring-friendly for ideographs).
    Trigram,
    /// Bounded ILIKE — a single CJK char or a too-short Latin token (trigram/FTS would be noise).
    Like,
}

/// True for Han / Hiragana / Katakana / Hangul code points (the scripts FTS5 word-tokenization fails on).
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF |   // CJK Unified Ideographs (Han)
        0x3040..=0x309F |   // Hiragana
        0x30A0..=0x30FF |   // Katakana
        0xAC00..=0xD7AF)    // Hangul syllables
}

/// Route a model-authored search pattern to the index path. Pure; the pattern is never mutated.
pub fn route_search(q: &str) -> SearchMode {
    let cjk_count = q.chars().filter(|c| is_cjk(*c)).count();
    let has_cjk = cjk_count > 0;
    let longest_latin = q.split_whitespace().map(|t| t.chars().count()).max().unwrap_or(0);
    match (has_cjk, cjk_count, longest_latin) {
        (true, n, _) if n >= 2 => SearchMode::Trigram, // CJK run ≥ 2 chars → pg_trgm
        (true, _, _) => SearchMode::Like,              // single CJK char → bounded LIKE
        (false, _, l) if l >= 3 => SearchMode::Tsvector, // a real Latin token → FTS
        _ => SearchMode::Like,                         // short Latin → bounded LIKE
    }
}

#[cfg(test)]
mod tests {
    use super::{route_search, SearchMode};
    #[test]
    fn routes_by_script_and_length() {
        assert_eq!(route_search("knowledge graph"), SearchMode::Tsvector); // Latin token ≥3
        assert_eq!(route_search("清华大学"), SearchMode::Trigram); // CJK run ≥2
        assert_eq!(route_search("数据库迁移"), SearchMode::Trigram);
        assert_eq!(route_search("学"), SearchMode::Like); // single CJK char
        assert_eq!(route_search("ab"), SearchMode::Like); // short Latin
        assert_eq!(route_search("部署"), SearchMode::Trigram); // 2 CJK chars (B-R24: ≥2 → trigram)
    }
}
