// QCue S2-R52 / RKM §7 #4 — realpath per-tenant path isolation. canonicalize → realpath → prefix-check
// → .md + size cap. Every vault read/write resolves through this guard against the per-tenant root
// `t/<tenant_id>/u/<user_id>/`; traversal, null bytes, symlink-escape, non-.md, and over-cap are
// rejected so one tenant can never touch another's files.
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("path escapes tenant root")]
    Escapes,
    #[error("null byte / illegal char")]
    Illegal,
    #[error("not a .md file")]
    NotMarkdown,
    #[error("exceeds size cap")]
    TooBig,
    #[error("io: {0}")]
    Io(String),
}

/// Resolve `rel` under `root`, rejecting traversal/null/symlink-escape/non-.md/over-cap.
pub fn resolve_in_root(root: &Path, rel: &str, size_cap: u64) -> Result<PathBuf, PathError> {
    if rel.contains('\0') || rel.contains("..") {
        return Err(PathError::Illegal);
    }
    if !rel.ends_with(".md") {
        return Err(PathError::NotMarkdown);
    }
    let root_real = std::fs::canonicalize(root).map_err(|e| PathError::Io(e.to_string()))?;
    let joined = root_real.join(rel);
    // realpath the deepest existing ancestor (file may not exist yet on a write).
    let check = if joined.exists() {
        std::fs::canonicalize(&joined).map_err(|e| PathError::Io(e.to_string()))?
    } else {
        let parent = joined.parent().ok_or(PathError::Escapes)?;
        let parent_real = std::fs::canonicalize(parent).map_err(|_| PathError::Escapes)?;
        parent_real.join(joined.file_name().ok_or(PathError::Escapes)?)
    };
    if !check.starts_with(&root_real) {
        return Err(PathError::Escapes);
    }
    if check.exists() {
        let meta = std::fs::metadata(&check).map_err(|e| PathError::Io(e.to_string()))?;
        if meta.len() > size_cap {
            return Err(PathError::TooBig);
        }
    }
    Ok(check)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::fs;
    #[test]
    fn rejects_traversal_and_cross_tenant_accepts_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t/A/u/X");
        fs::create_dir_all(root.join("entities")).unwrap();
        fs::write(root.join("entities/foo.md"), b"x").unwrap();
        // valid in-root .md → Ok
        assert!(resolve_in_root(&root, "entities/foo.md", 1024).is_ok());
        // traversal → Err
        assert!(resolve_in_root(&root, "../../B/u/Y/entities/foo.md", 1024).is_err());
        // null byte → Err
        assert!(resolve_in_root(&root, "entities/foo\0.md", 1024).is_err());
        // non-.md → Err
        assert!(resolve_in_root(&root, "entities/foo.txt", 1024).is_err());
        // over size cap → Err (the existing 1-byte file with cap 0)
        assert!(resolve_in_root(&root, "entities/foo.md", 0).is_err());
        // absolute path that escapes the root → Err
        assert!(resolve_in_root(&root, "/etc/passwd.md", 1024).is_err());
    }
}
