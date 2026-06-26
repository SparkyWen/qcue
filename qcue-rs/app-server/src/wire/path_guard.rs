//! QCue S3-R46 / B-R28 — realpath + prefix-check tenant path guard. Stops tenant A reaching tenant B
//! and rejects traversal / null-byte / non-markdown / absolute paths before any filesystem touch.
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PathError {
    #[error("null byte")]
    NullByte,
    #[error("traversal")]
    Traversal,
    #[error("not markdown")]
    NotMarkdown,
    #[error("escapes root")]
    EscapesRoot,
    #[error("absolute")]
    Absolute,
}

/// Resolve a `.md` relative path under `root`, realpath-checking the deepest existing ancestor against
/// the canonical root so a symlink cannot escape. The file itself may not exist yet (writes).
pub fn resolve_under_root(root: &Path, rel: &str) -> Result<PathBuf, PathError> {
    resolve_under_root_ext(root, rel, &["md"])
}

/// Generalized guard: same realpath+prefix logic, but accepts any extension in `allowed_exts` (DRY —
/// the `.jsonl` capture writer reuses this with `["jsonl"]`; the strict wiki-read path uses `["md"]`).
pub fn resolve_under_root_ext(root: &Path, rel: &str, allowed_exts: &[&str]) -> Result<PathBuf, PathError> {
    if rel.contains('\0') {
        return Err(PathError::NullByte);
    }
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err(PathError::Absolute);
    }
    if rel.split('/').any(|c| c == "..") {
        return Err(PathError::Traversal);
    }
    let ext_ok = allowed_exts.iter().any(|e| {
        rel.rsplit('.').next().map(|got| got.eq_ignore_ascii_case(e)).unwrap_or(false)
            && rel.contains('.')
    });
    if !ext_ok {
        return Err(PathError::NotMarkdown);
    }
    let joined = root.join(rel);
    // realpath of the deepest existing ancestor (the file may not exist yet for writes).
    let mut probe = joined.clone();
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    loop {
        if let Ok(c) = probe.canonicalize() {
            if !c.starts_with(&canon_root) {
                return Err(PathError::EscapesRoot);
            }
            break;
        }
        match probe.parent() {
            Some(par) => probe = par.to_path_buf(),
            None => break,
        }
    }
    if !joined.starts_with(root) {
        return Err(PathError::EscapesRoot);
    }
    Ok(joined)
}
