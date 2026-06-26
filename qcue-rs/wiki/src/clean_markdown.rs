// QCue S2 — `clean_markdown` (PORT utils.ts cleanMarkdownResponse, deterministic core). Pure, no IO.
//
// Strips reasoning/thinking blocks (<think>…</think>, <thinking>…</thinking>) emitted by reasoning
// models, then unwraps a single ```markdown / ```md / ``` code-fence wrapper. This is the LLM-free,
// deterministic half of the wiki-engine markdown cleaner (the auto-frontmatter-prefix heuristics are
// LLM-coupled and deferred to the ingest milestone).

/// Remove `<tag>…</tag>` blocks (case-insensitive, non-greedy) for the given tag name.
fn strip_block(input: &str, tag: &str) -> String {
    let open_lower = format!("<{tag}");
    let close_lower = format!("</{tag}>");
    let mut out = String::with_capacity(input.len());
    let lower = input.to_lowercase();
    let mut i = 0usize;
    while i < input.len() {
        if let Some(rel) = lower[i..].find(&open_lower) {
            let open_at = i + rel;
            // require the open tag to be followed by '>' or whitespace/attrs ending in '>'
            if let Some(gt_rel) = lower[open_at..].find('>') {
                let after_open = open_at + gt_rel + 1;
                if let Some(close_rel) = lower[after_open..].find(&close_lower) {
                    let close_end = after_open + close_rel + close_lower.len();
                    out.push_str(&input[i..open_at]);
                    i = close_end;
                    continue;
                }
            }
        }
        out.push_str(&input[i..]);
        break;
    }
    out
}

/// Strip thinking blocks and unwrap a markdown code-fence wrapper. Deterministic.
pub fn clean_markdown(response: &str) -> String {
    let mut cleaned = response.trim().to_string();
    cleaned = strip_block(&cleaned, "think");
    cleaned = strip_block(&cleaned, "thinking");
    let mut cleaned = cleaned.trim().to_string();

    // Unwrap a leading ```markdown / ```md / ``` fence and a trailing ``` if present.
    for marker in ["```markdown", "```md", "```"] {
        if let Some(rest) = cleaned.strip_prefix(marker) {
            cleaned = rest.trim_start_matches(['\n', ' ', '\t']).to_string();
            break;
        }
    }
    if let Some(rest) = cleaned.strip_suffix("```") {
        cleaned = rest.trim_end_matches(['\n', ' ', '\t']).to_string();
    }
    cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    #[test]
    fn strips_thinking_and_unwraps_fences() {
        assert_eq!(
            clean_markdown("<think>reasoning…</think>\n# Title\nBody"),
            "# Title\nBody"
        );
        assert_eq!(
            clean_markdown("```markdown\n# Title\nBody\n```"),
            "# Title\nBody"
        );
        assert_eq!(clean_markdown("```\nplain\n```"), "plain");
        // clean content passes through (trimmed)
        assert_eq!(clean_markdown("  # Already clean\n"), "# Already clean");
        // <thinking> variant
        assert_eq!(clean_markdown("<thinking>x</thinking>\nY"), "Y");
    }
}
