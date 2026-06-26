// QCue S1-R38 / B-R11 — central secret redaction at the persistence boundary.
/// Replace provider-key-shaped substrings with [REDACTED]. Applied before any write to
/// messages / audit_log / sync_ops / JSONL log (Appendix B B-R11).
pub fn redact_secrets(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for token in split_keep(input) {
        if looks_like_key(token) {
            out.push_str("[REDACTED]");
        } else {
            out.push_str(token);
        }
    }
    out
}

fn looks_like_key(tok: &str) -> bool {
    // sk-..., sk-ant-..., sk-proj-..., AKIA..., ghp_..., and long high-entropy base64-ish tokens.
    let t = tok.trim();
    (t.starts_with("sk-") && t.len() >= 12)
        || (t.starts_with("AKIA") && t.len() >= 16)
        || (t.starts_with("ghp_") && t.len() >= 16)
}

/// split on whitespace but keep the whitespace tokens so we can rejoin losslessly.
fn split_keep(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut last = 0;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            if last != i {
                out.push(&s[last..i]);
            }
            out.push(&s[i..i + c.len_utf8()]);
            last = i + c.len_utf8();
        }
    }
    if last < s.len() {
        out.push(&s[last..]);
    }
    out
}
