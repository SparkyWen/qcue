// QCue S2-R35..R40 — pure-SQL lint scanners over the PG link-graph (B-R16). No LLM, no markdown reads
// (pitfall #12). Every scan is index-backed and tenant-scoped (RLS); destructive fixes (later) must be
// reversible, but these scanners are strictly READ-ONLY.
use store::wiki_repo::WikiRepo;
use uuid::Uuid;

pub const MIN_SUBSTANTIVE_CHARS: i32 = 50; // constants.ts:28

pub struct Scanners {
    repo: WikiRepo,
}
impl Scanners {
    pub fn new(repo: WikiRepo) -> Self {
        Self { repo }
    }

    /// S2-R35 — dead links: wiki_links with target_page_id IS NULL (uses wiki_links_dead_idx).
    pub async fn dead_links(&self, tenant: Uuid) -> sqlx::Result<Vec<Uuid>> {
        self.repo
            .scan_ids(tenant, "SELECT id FROM wiki_links WHERE tenant_id=$1 AND target_page_id IS NULL")
            .await
    }

    /// S2-R36 — orphans: pages with zero incoming links (alias-aware via target_page_id resolution at write).
    pub async fn orphans(&self, tenant: Uuid) -> sqlx::Result<Vec<Uuid>> {
        self.repo
            .scan_ids(
                tenant,
                "SELECT p.id FROM wiki_pages p WHERE p.tenant_id=$1 AND p.deleted_at IS NULL \
                 AND NOT EXISTS (SELECT 1 FROM wiki_links l WHERE l.tenant_id=$1 AND l.target_page_id=p.id)",
            )
            .await
    }

    /// S2-R37 — empty pages: char_len < MIN_SUBSTANTIVE_CHARS (the stored length, never a body read).
    pub async fn empty_pages(&self, tenant: Uuid) -> sqlx::Result<Vec<Uuid>> {
        self.repo
            .scan_ids_bind(
                tenant,
                "SELECT id FROM wiki_pages WHERE tenant_id=$1 AND deleted_at IS NULL AND char_len < $2",
                MIN_SUBSTANTIVE_CHARS,
            )
            .await
    }

    /// S2-R38 — missing aliases: entity/concept pages with empty aliases (run FIRST in fix order).
    pub async fn missing_aliases(&self, tenant: Uuid) -> sqlx::Result<Vec<Uuid>> {
        self.repo
            .scan_ids(
                tenant,
                "SELECT id FROM wiki_pages WHERE tenant_id=$1 AND deleted_at IS NULL \
                 AND type IN ('entity','concept') AND cardinality(aliases)=0",
            )
            .await
    }

    /// S2-R39 — tag violations: tags outside the active per-tenant vocabulary.
    pub async fn tag_violations(&self, tenant: Uuid, vocab: &[String]) -> sqlx::Result<Vec<Uuid>> {
        self.repo
            .scan_ids_tags(
                tenant,
                "SELECT id FROM wiki_pages WHERE tenant_id=$1 AND deleted_at IS NULL \
                 AND EXISTS (SELECT 1 FROM unnest(tags) t WHERE t <> ALL($2))",
                vocab,
            )
            .await
    }
}

/// S2-R40 — strip folder-prefix self-duplication in a basename (`entities/entities/X` → `entities/X`).
/// The complement of the link-sanitizer's body fix (linksan), applied to a page path/basename — the
/// polluted-basename lint hit's deterministic repair (no LLM, reversible: it only de-duplicates).
pub fn fix_polluted_basename(p: &str) -> String {
    for folder in ["entities", "concepts", "sources"] {
        let doubled = format!("{folder}/{folder}/");
        if let Some(rest) = p.strip_prefix(&doubled) {
            return format!("{folder}/{rest}");
        }
    }
    p.to_string()
}

#[cfg(test)]
mod polluted_tests {
    use super::fix_polluted_basename;
    #[test]
    fn detects_and_fixes_folder_prefix_duplication() {
        // S2-R40 — entities/entities/X is a polluted basename; the fix strips the duplicated prefix.
        assert_eq!(fix_polluted_basename("entities/entities/X"), "entities/X");
        assert_eq!(fix_polluted_basename("concepts/foo"), "concepts/foo"); // clean untouched
    }
}
