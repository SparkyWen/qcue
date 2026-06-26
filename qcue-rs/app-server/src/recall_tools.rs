// QCue v0.1.1 — the REAL recall tool handler (Appendix A: recall is agentic, not a fixed retrieval).
//
// `router::run_turn` advertises `recall_search`/`read_page`/`read_lines` to the model; when the model
// authors a call, `ToolDispatcher` routes it here. `recall_search` runs the actual RLS-scoped search
// (`ideas::run_recall_search` over `SearchRepo`) with the model's pattern passed through VERBATIM
// (A-R13) and returns bookended hits + conservative citations the model can cite. `read_page`/
// `read_lines` are realpath-guarded reads confined to the tenant's vault root (no '/'-escape, no '..').
use crate::web_tool::WebClient;
use async_trait::async_trait;
use ideas::recall::search_tool::{infer_mode, run_recall_search, RecallArgs};
use router::tools::ToolExec;
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use store::search_repo::SearchRepo;
use uuid::Uuid;

/// Tenant-bound recall tools. Constructed per turn in `RouterWikiLlm::create_message` (which knows the
/// tenant), so every search/read is already scoped — RLS is the belt, this binding is the suspenders.
/// `web` is the live-internet client; `None` keeps recall offline (web tools then return a clear error
/// instead of silently failing), so Dream and the keyless paths stay network-free.
pub struct RecallToolExec {
    tenant: Uuid,
    search: SearchRepo,
    vault_root: PathBuf,
    current_session: Option<Uuid>,
    web: Option<Arc<WebClient>>,
}

impl RecallToolExec {
    pub fn new(
        tenant: Uuid,
        pool: PgPool,
        vault_root: PathBuf,
        current_session: Option<Uuid>,
        web: Option<Arc<WebClient>>,
    ) -> Self {
        Self { tenant, search: SearchRepo::new(pool), vault_root, current_session, web }
    }

    /// Run the model-authored search and render the hits as a compact, citable tool result.
    async fn recall_search(&self, arguments: &str) -> Result<String, String> {
        let v: serde_json::Value = serde_json::from_str(arguments)
            .map_err(|e| format!("invalid recall_search arguments: {e}"))?;
        let pattern = v.get("pattern").and_then(|p| p.as_str()).unwrap_or("").trim().to_string();
        if pattern.is_empty() {
            return Err("recall_search requires a non-empty `pattern`".into());
        }
        let args = RecallArgs {
            mode: infer_mode(&pattern, false), // inferred from arg shape; the pattern is NEVER rewritten (A-R13)
            pattern: pattern.clone(),
            current_session: self.current_session,
        };
        let (mode, hits) =
            run_recall_search(self.tenant, &self.search, args).await.map_err(|e| e.to_string())?;
        if hits.is_empty() {
            return Ok(format!("No results found for \"{pattern}\" (mode: {mode:?})."));
        }
        let mut out = format!("Found {} result(s) for \"{pattern}\" (mode: {mode:?}):\n", hits.len());
        for (i, h) in hits.iter().take(10).enumerate() {
            let cite = h
                .citation
                .as_ref()
                .map(|c| format!("{}:{}", c.rel_path, c.start_line))
                .unwrap_or_else(|| "(no citation)".into());
            out.push_str(&format!("\n[{}] {cite}\n", i + 1));
            if let Some(g) = &h.goal {
                out.push_str(&format!("  goal: {}\n", truncate(g, 200)));
            }
            if let Some(c) = &h.conclusion {
                out.push_str(&format!("  conclusion: {}\n", truncate(c, 200)));
            }
            out.push_str(&format!("  window: {}\n", truncate(&h.window, 600)));
        }
        Ok(out)
    }

    /// Read a whole page body, realpath-confined to the vault root. The model passes `{slug}`; we try
    /// `<slug>.md` and the Karpathy type subdirs (`entities/`, `concepts/`, `sources/`).
    async fn read_page(&self, arguments: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(arguments).map_err(|e| format!("invalid read_page arguments: {e}"))?;
        let slug = v.get("slug").and_then(|s| s.as_str()).unwrap_or("").trim();
        if slug.is_empty() {
            return Err("read_page requires a `slug`".into());
        }
        match self.resolve_page(slug) {
            Some(body) => Ok(truncate(&body, 8000)),
            None => Ok(format!("page not found: {slug}")),
        }
    }

    /// Read a line window `[start, end]` (1-based, inclusive) of a page, realpath-confined.
    async fn read_lines(&self, arguments: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(arguments).map_err(|e| format!("invalid read_lines arguments: {e}"))?;
        let slug = v.get("slug").and_then(|s| s.as_str()).unwrap_or("").trim();
        if slug.is_empty() {
            return Err("read_lines requires a `slug`".into());
        }
        let start = v.get("start").and_then(|n| n.as_u64()).unwrap_or(1).max(1) as usize;
        let end = v.get("end").and_then(|n| n.as_u64()).unwrap_or(start as u64 + 40) as usize;
        match self.resolve_page(slug) {
            Some(body) => {
                let window: Vec<String> = body
                    .lines()
                    .enumerate()
                    .filter(|(i, _)| *i + 1 >= start && *i < end)
                    .map(|(i, l)| format!("{}: {l}", i + 1))
                    .collect();
                Ok(if window.is_empty() { "(no lines in range)".into() } else { window.join("\n") })
            }
            None => Ok(format!("page not found: {slug}")),
        }
    }

    /// Resolve a slug to a page body, confined to the vault root (the realpath guard rejects any path
    /// that escapes the root — defense in depth over the conservative-citation belt).
    fn resolve_page(&self, slug: &str) -> Option<String> {
        if slug.contains("..") {
            return None;
        }
        let candidates = [
            self.vault_root.join(format!("{slug}.md")),
            self.vault_root.join(format!("entities/{slug}.md")),
            self.vault_root.join(format!("concepts/{slug}.md")),
            self.vault_root.join(format!("sources/{slug}.md")),
        ];
        for c in candidates {
            if self.under_root(&c)
                && let Ok(body) = std::fs::read_to_string(&c)
            {
                return Some(body);
            }
        }
        None
    }

    /// True iff `p` canonicalizes to a path under the (canonicalized) vault root (no traversal escape).
    fn under_root(&self, p: &Path) -> bool {
        match (std::fs::canonicalize(&self.vault_root), std::fs::canonicalize(p)) {
            (Ok(root), Ok(real)) => real.starts_with(root),
            _ => false,
        }
    }
}

#[async_trait]
impl ToolExec for RecallToolExec {
    async fn call(&self, name: &str, arguments: &str) -> Result<String, String> {
        match name {
            "recall_search" => self.recall_search(arguments).await,
            "read_page" => self.read_page(arguments).await,
            "read_lines" => self.read_lines(arguments).await,
            // Live-internet tools — executed only when a web client is wired (recall); the SSRF guard +
            // the untrusted-result marking live in `web_tool`. Offline paths return a clear, recoverable
            // error AS the tool result (the model can fall back to its own knowledge, never hard-fails).
            "web_fetch" => match &self.web {
                Some(w) => w.fetch(arguments).await,
                None => Err("web_fetch is disabled (no internet access in this context)".into()),
            },
            "web_search" => match &self.web {
                Some(w) => w.search(arguments).await,
                None => Err("web_search is disabled (no internet access in this context)".into()),
            },
            other => Err(format!("unknown recall tool: {other}")),
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}
