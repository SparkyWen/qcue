// QCue S2-R49/R4/R5/R6/R37 — THE single body-write site (pitfall #11). No other code path writes a
// page body. Every write: sanitizes links → parses the link-graph → upserts the mirror row with a
// SYSTEM-set char_len → re-derives wiki_links → writes the markdown body under the per-tenant root.
// LLM-supplied created/updated/reviewed are deliberately IGNORED (system/DB-controlled, B-R7/S2-R6).
use crate::page::PageType;
use crate::path_guard::resolve_in_root;
use crate::sandbox::TenantSandbox;
use linksan::{parse_wikilinks, sanitize_links};
use sha2::{Digest, Sha256};
use store::wiki_repo::{PageRow, PageUpsert, WikiRepo};
use uuid::Uuid;

const BODY_SIZE_CAP: u64 = 1_000_000;

/// SYNC-D6: lowercase-hex sha-256 of a string (the sanitized body content hash).
fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let digest = h.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// A page write request. `body` is the raw (possibly LLM-authored, possibly polluted) markdown body;
/// it is sanitized before anything is persisted. `llm_created`/`llm_reviewed` are accepted only so the
/// gate can explicitly DROP them — created/updated are system-set, reviewed is DB-controlled (S2-R6).
pub struct PageWrite {
    pub r#type: String,
    pub slug: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub summary: String,
    pub source_ids: Vec<Uuid>,
    pub body: String,
    pub llm_created: Option<String>, // STRIPPED — created is system-set (S2-R6)
    pub llm_reviewed: Option<bool>,  // STRIPPED on create — reviewed stays DB-controlled (S2-R6)
}

pub struct WikiWriteGate {
    repo: WikiRepo,
    sandbox: TenantSandbox,
}
impl WikiWriteGate {
    pub fn new(repo: WikiRepo, sandbox: TenantSandbox) -> Self {
        Self { repo, sandbox }
    }

    /// The one write path: sanitize → quota check → parse links → upsert mirror (system char_len +
    /// content_hash + bumped sync_version) → replace link-graph → write body. Returns the page id.
    pub async fn write_page(&self, tenant: Uuid, w: PageWrite) -> anyhow::Result<Uuid> {
        // (a) sanitize links (S2-R49a) — the central pollution defense (pitfall #11).
        let sanitized = sanitize_links(&w.body);
        // (c) IGNORE w.llm_created / w.llm_reviewed: created/updated are system-set, reviewed is
        //     DB-controlled (S2-R6). char_len is the true sanitized-body char count (S2-R37, pitfall #12).
        let _ = (&w.llm_created, &w.llm_reviewed);
        let char_len = i32::try_from(sanitized.chars().count()).unwrap_or(i32::MAX);
        // SYNC-D6: content_hash = sha-256 hex of the SANITIZED body (the persisted content). Set in the
        // mirror upsert (which also bumps sync_version) so a warm sync client can skip a body whose
        // hash it already holds.
        let content_hash = sha256_hex(&sanitized);
        // SBX-R5: bound the tenant's vault footprint (disk-fill DoS). Generous, env-tunable caps.
        let (pages, bytes) = self.repo.vault_usage(tenant).await?;
        let q = &self.sandbox.quota;
        if pages as usize >= q.max_pages
            || (bytes as u64).saturating_add(char_len as u64) > q.max_bytes
        {
            anyhow::bail!(
                "tenant vault quota exceeded ({pages} pages / {bytes} bytes; caps {} / {})",
                q.max_pages,
                q.max_bytes
            );
        }
        // resolve the vault path under the per-tenant root (typed folder for entity/concept/source).
        let folder = PageType::parse(&w.r#type).and_then(PageType::folder).unwrap_or("");
        let rel = if folder.is_empty() {
            format!("{}.md", w.slug)
        } else {
            format!("{folder}/{}.md", w.slug)
        };
        // the file may not exist yet; ensure the directory exists so the path-guard can realpath it.
        let abs_dir = if folder.is_empty() {
            self.sandbox.vault_root.clone()
        } else {
            self.sandbox.vault_root.join(folder)
        };
        tokio::fs::create_dir_all(&abs_dir).await?;
        let path = resolve_in_root(&self.sandbox.vault_root, &rel, BODY_SIZE_CAP)?;
        let body_ref = path.to_string_lossy().to_string();
        // Write the sanitized markdown body to the vault (the content source-of-truth) FIRST, so a failed
        // disk write can never leave a committed mirror row pointing at a missing file (the worse half of
        // the dual representation to corrupt; an orphan body file is invisible to lint and harmless).
        tokio::fs::write(&path, sanitized.as_bytes()).await?;
        // (d) upsert the mirror row (now that the body exists) so self-link target resolution finds it.
        let id = self
            .repo
            .upsert_page(
                tenant,
                &PageUpsert {
                    r#type: w.r#type.clone(),
                    slug: w.slug.clone(),
                    title: w.title.clone(),
                    aliases: w.aliases.clone(),
                    tags: w.tags.clone(),
                    summary: w.summary.clone(),
                    char_len,
                    body_ref: body_ref.clone(),
                    source_ids: w.source_ids.clone(),
                    content_hash: content_hash.clone(),
                },
            )
            .await?;
        // (b) parse [[wikilinks]] → re-derive wiki_links (target_page_id by slug/alias; NULL ⇒ dead).
        let links = parse_wikilinks(&sanitized);
        self.repo.replace_links(tenant, id, &links).await?;
        Ok(id)
    }

    /// Read a page's markdown body from the vault (resolves via the stored body_ref). Lint never calls
    /// this — it is for content callers only (pitfall #12: lint reads Postgres, not markdown).
    pub async fn read_body(&self, tenant: Uuid, id: Uuid) -> anyhow::Result<String> {
        let p = self.repo.page(tenant, id).await?;
        Ok(tokio::fs::read_to_string(&p.body_ref).await?)
    }

    /// Fetch the structured mirror row (frontmatter projection) for a page.
    pub async fn page(&self, tenant: Uuid, id: Uuid) -> anyhow::Result<PageRow> {
        Ok(self.repo.page(tenant, id).await?)
    }
}
