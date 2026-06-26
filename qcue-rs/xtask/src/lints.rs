//! Build-graph lints run in CI. QCue S1-R1, S1-R2.
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub fn workspace_root() -> PathBuf {
    // `xtask/` is always a child of the workspace root; fall back to "." if not.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The locked layering order. Index = layer height; a crate may depend only on
/// crates at a strictly lower height (plus `secrets`/`store` which hang off router).
const LAYER_ORDER: &[&str] = &["protocol", "http", "llm-api", "providers", "router"];

fn manifest_deps(crate_dir: &Path) -> HashSet<String> {
    let toml = match fs::read_to_string(crate_dir.join("Cargo.toml")) {
        Ok(t) => t,
        Err(_) => return HashSet::new(),
    };
    // crude but sufficient: collect bare `name` tokens that are workspace crates,
    // only within the [dependencies] section.
    let mut out = HashSet::new();
    let mut in_deps = false;
    for line in toml.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_deps = l == "[dependencies]";
            continue;
        }
        if in_deps {
            // `name = ...` and `name.workspace = true` both reduce to the bare crate name.
            if let Some(name) = l.split(['=', ' ', '.']).next() {
                let name = name.trim();
                if !name.is_empty() {
                    out.insert(name.to_string());
                }
            }
        }
    }
    out
}

pub fn check_layering_law(root: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    for (height, krate) in LAYER_ORDER.iter().enumerate() {
        let deps = manifest_deps(&root.join(krate));
        for (upper_height, upper) in LAYER_ORDER.iter().enumerate() {
            if upper_height > height && deps.contains(*upper) {
                violations.push(format!("{krate} -> {upper} (upward edge)"));
            }
        }
    }
    violations
}

pub fn check_protocol_deps_minimal(root: &Path) -> Vec<String> {
    let allow: HashSet<&str> = [
        "serde",
        "serde_json",
        "thiserror",
        "uuid",
        "chrono",
        "ts-rs",
        "schemars",
        "futures-core",
    ]
    .into_iter()
    .collect();
    let mut problems = Vec::new();
    for dep in manifest_deps(&root.join("protocol")) {
        if !allow.contains(dep.as_str()) {
            problems.push(format!("protocol depends on non-allowlisted `{dep}`"));
        }
    }
    // no async/tokio/reqwest/sqlx tokens in protocol/src (CODE only; line comments are
    // stripped first so doc-comments that merely *name* the banned crates don't trip the lint).
    let src = root.join("protocol/src");
    for entry in walk(&src) {
        let body = fs::read_to_string(&entry).unwrap_or_default();
        let code: String = body
            .lines()
            .map(|line| match line.find("//") {
                Some(idx) => &line[..idx],
                None => line,
            })
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in ["async fn", "tokio::", "reqwest", "sqlx"] {
            if code.contains(forbidden) {
                problems.push(format!("{}: forbidden token `{forbidden}`", entry.display()));
            }
        }
    }
    problems
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    out
}
