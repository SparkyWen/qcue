// QCue S2-R8 — deterministic slug (ported from utils.ts slugify).
//
// CJK is kept verbatim (no ASCII-folding); ASCII letters are lowercased; runs of non-alphanumerics
// collapse to a single hyphen; leading/trailing hyphens are trimmed; empty → "untitled".

pub fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut prev_hyphen = false;
    for ch in title.trim().chars() {
        if ch.is_alphanumeric() {
            for low in ch.to_lowercase() {
                out.push(low);
            }
            prev_hyphen = false;
        } else if !prev_hyphen && !out.is_empty() {
            out.push('-');
            prev_hyphen = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "untitled".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::slugify;
    #[test]
    fn slugify_normalizes_titles() {
        assert_eq!(slugify("Tsinghua University"), "tsinghua-university");
        assert_eq!(slugify("  Data  Structures & Algorithms! "), "data-structures-algorithms");
        assert_eq!(slugify("C++ / Rust"), "c-rust");
        assert_eq!(slugify("清华大学"), "清华大学"); // CJK kept, spaces→hyphen, no ASCII-folding
        assert_eq!(slugify("--Already--Hyphen--"), "already-hyphen");
        assert_eq!(slugify(""), "untitled");
    }
}
