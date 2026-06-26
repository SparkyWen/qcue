// QCue S2-R2 — JSON robustness utils (PORT wiki-engine §9: parse_json_response, prefill, truncation-retry).
//
// Mirrors `router::structured::parse_json_response` (strip fences/`<think>` + brace-count) without taking
// a dependency on the router crate (wiki/ideas are a lower sibling layer): the extract loop must isolate
// the first balanced JSON object out of a fenced / reasoning-prefixed model response.
use serde_json::Value;

pub const PREFILL_OPEN_BRACE: &str = "{";

/// Strip ```json fences and <think>…</think>, then brace-count to isolate the first complete JSON object.
pub fn parse_json_response(raw: &str) -> Result<Value, serde_json::Error> {
    let mut s = raw.trim();
    // strip <think> preambles
    if let Some(idx) = s.rfind("</think>") {
        s = s[idx + "</think>".len()..].trim();
    }
    // strip code fences
    let s = s.trim_start_matches("```json").trim_start_matches("```").trim();
    let s = s.strip_suffix("```").unwrap_or(s).trim();
    // brace-count from the first '{' to its matching '}'
    if let Some(start) = s.find('{') {
        let bytes = s.as_bytes();
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        for (i, &b) in bytes.iter().enumerate().skip(start) {
            match b {
                b'"' if !esc => in_str = !in_str,
                b'\\' if in_str => {
                    esc = !esc;
                    continue;
                }
                b'{' if !in_str => depth += 1,
                b'}' if !in_str => {
                    depth -= 1;
                    if depth == 0 {
                        return serde_json::from_str(&s[start..=i]);
                    }
                }
                _ => {}
            }
            esc = false;
        }
    }
    serde_json::from_str(s)
}

/// Truncation-retry budget: double max_tokens when finish_reason == Length. Caller enforces "once".
pub fn next_max_tokens_on_truncation(current: u32) -> Option<u32> {
    Some(current.saturating_mul(2))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    #[test]
    fn parse_strips_fences_and_think_and_brace_counts() {
        let fenced = "```json\n{\"a\":1}\n```";
        assert_eq!(parse_json_response(fenced).unwrap()["a"], 1);
        let thunk = "<think>reasoning…</think>\n{\"b\":2}";
        assert_eq!(parse_json_response(thunk).unwrap()["b"], 2);
        // trailing prose after the JSON object → brace-count isolates the object
        let trailing = "{\"c\":3} and that's my answer.";
        assert_eq!(parse_json_response(trailing).unwrap()["c"], 3);
    }
    #[test]
    fn truncation_doubles_max_tokens_once() {
        let plan = next_max_tokens_on_truncation(1000);
        assert_eq!(plan, Some(2000));
    }
}
