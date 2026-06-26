//! QCue S3-R21 / B-R11 — central secret redaction at the persistence boundary.
//! Skeleton patterns: scrub bearer tokens + provider key prefixes so a secret never reaches
//! audit_log.detail or a log line. (Vault `key_hint` prefix patterns are added with the vault API.)
use serde_json::Value;

/// Redact obvious secret material from a free-text string (best-effort, never panics).
pub fn redact_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for token in s.split_inclusive(|c: char| c.is_whitespace()) {
        let trimmed = token.trim_end();
        if looks_secret(trimmed) {
            out.push_str("[REDACTED]");
            // preserve the trailing whitespace the inclusive split kept
            out.push_str(&token[trimmed.len()..]);
        } else {
            out.push_str(token);
        }
    }
    out
}

fn looks_secret(s: &str) -> bool {
    const PREFIXES: &[&str] = &["sk-", "sk_live_", "sk_test_", "AKIA", "Bearer", "ghp_", "xoxb-"];
    PREFIXES.iter().any(|p| s.starts_with(p)) && s.len() > 8
}

/// Recursively redact secret-looking string values + secret-keyed fields in a JSON tree.
pub fn redact_json(v: &mut Value) {
    match v {
        Value::String(s) => {
            let r = redact_str(s);
            if &r != s {
                *s = r;
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_json(item);
            }
        }
        Value::Object(map) => {
            for (k, val) in map.iter_mut() {
                if is_secret_key(k) {
                    *val = Value::String("[REDACTED]".into());
                } else {
                    redact_json(val);
                }
            }
        }
        _ => {}
    }
}

fn is_secret_key(k: &str) -> bool {
    const KEYS: &[&str] = &["password", "api_key", "apikey", "secret", "token", "authorization", "key_ciphertext"];
    let kl = k.to_ascii_lowercase();
    KEYS.iter().any(|s| kl.contains(s))
}
