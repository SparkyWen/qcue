// QCue S2-R11/R26/R30, A-R18 — materialize index.md from wiki_pages (no body reads) + truncate to 200
// lines / 25KB. The index is the index-first retrieval substrate AND the dedup-gate input; it is
// regenerated on every ingest write (wiki-engine.ts:522). Read only from the PG mirror — never the
// markdown bodies (pitfall #12).
use store::wiki_repo::WikiRepo;
use uuid::Uuid;

pub const MAX_INDEX_LINES: usize = 200;
pub const MAX_INDEX_BYTES: usize = 25_000;
pub const MAX_LINE_CHARS: usize = 150;

/// Build the catalog from PG. Each line: `- [[slug|Title]] — one-line hook` (≤150 chars). Empty wiki →
/// an explicit "(wiki is empty)" sentinel so the dedup/synthesis prompts can branch on it.
pub async fn regenerate_index(tenant: Uuid, repo: &WikiRepo) -> anyhow::Result<String> {
    let rows = repo.catalog_rows(tenant).await?;
    if rows.is_empty() {
        return Ok("# Index\n\n(wiki is empty)\n".to_string());
    }
    let mut out = String::from("# Index\n\n");
    for (slug, title, summary, aliases) in rows {
        let hook: String =
            summary.chars().take(MAX_LINE_CHARS.saturating_sub(slug.len() + 12)).collect();
        let alias_note = if aliases.is_empty() {
            String::new()
        } else {
            format!(" (aliases: {})", aliases.join(", "))
        };
        let mut line = format!("- [[{slug}|{title}]] — {hook}{alias_note}");
        if line.chars().count() > MAX_LINE_CHARS {
            line = line.chars().take(MAX_LINE_CHARS).collect();
        }
        out.push_str(&line);
        out.push('\n');
    }
    Ok(truncate_index(&out))
}

/// A-R18 — truncate at 200 lines then 25KB at a newline boundary, appending a WARNING naming the cap.
pub fn truncate_index(idx: &str) -> String {
    let mut lines: Vec<&str> = idx.lines().collect();
    let mut warned = String::new();
    if lines.len() > MAX_INDEX_LINES {
        lines.truncate(MAX_INDEX_LINES);
        warned = format!("\n> WARNING: index truncated at {MAX_INDEX_LINES} lines.");
    }
    let mut joined = lines.join("\n");
    if joined.len() > MAX_INDEX_BYTES {
        let cut = joined[..MAX_INDEX_BYTES].rfind('\n').unwrap_or(MAX_INDEX_BYTES);
        joined.truncate(cut);
        warned = format!("\n> WARNING: index truncated at {MAX_INDEX_BYTES} bytes.");
    }
    format!("{joined}{warned}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::truncate_index;
    #[test]
    fn truncates_at_line_and_byte_caps_with_warning() {
        let big = (0..300).map(|i| format!("- [[p{i}]] — hook")).collect::<Vec<_>>().join("\n");
        let out = truncate_index(&big);
        assert!(out.lines().count() <= 201); // 200 + a WARNING line (A-R18)
        assert!(out.contains("WARNING"));
        assert!(out.len() <= 25_000);
    }
}
