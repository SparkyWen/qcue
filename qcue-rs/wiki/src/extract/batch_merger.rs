// QCue S2-R15 — case-insensitive name+alias merge/dedup (PORT batch-merger.ts:46-104).
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedItem {
    pub name: String,
    pub aliases: Vec<String>,
}

fn keys(item: &ExtractedItem) -> Vec<String> {
    let mut v = vec![item.name.to_lowercase()];
    v.extend(item.aliases.iter().map(|a| a.to_lowercase()));
    v
}

/// Merge a new batch into the accumulator, folding case-insensitive name/alias collisions.
pub fn merge_batch_results(mut acc: Vec<ExtractedItem>, next: Vec<ExtractedItem>) -> Vec<ExtractedItem> {
    for cand in next {
        let cand_keys = keys(&cand);
        let hit = acc.iter_mut().find(|existing| {
            let ek = keys(existing);
            cand_keys.iter().any(|k| ek.contains(k))
        });
        match hit {
            Some(existing) => {
                for a in cand.aliases {
                    let al = a.to_lowercase();
                    if existing.name.to_lowercase() != al
                        && !existing.aliases.iter().any(|x| x.to_lowercase() == al)
                    {
                        existing.aliases.push(a);
                    }
                }
            }
            None => acc.push(cand),
        }
    }
    acc
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    fn item(name: &str, aliases: &[&str]) -> ExtractedItem {
        ExtractedItem { name: name.into(), aliases: aliases.iter().map(|s| s.to_string()).collect() }
    }
    #[test]
    fn merge_dedups_case_insensitive_by_name_and_alias() {
        let acc = vec![item("Rust", &["rustlang"])];
        let next = vec![item("RUST", &["rust-lang"]), item("Tokio", &["async runtime"])];
        let merged = merge_batch_results(acc, next);
        assert_eq!(merged.len(), 2); // RUST folds into Rust
        let rust = merged.iter().find(|i| i.name == "Rust").unwrap();
        assert!(rust.aliases.contains(&"rust-lang".to_string())); // alias union
        assert!(merged.iter().any(|i| i.name == "Tokio"));
    }
    #[test]
    fn alias_collision_folds_into_existing() {
        let acc = vec![item("Data Structures", &["DSA"])];
        let next = vec![item("DSA", &[])]; // name matches an existing alias
        let merged = merge_batch_results(acc, next);
        assert_eq!(merged.len(), 1);
    }
}
