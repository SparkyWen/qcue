// QCue S2-R21/S2-R62 — tenant-scoped CRUD over wiki_pages/wiki_links. Every op takes tenant_id first
// and runs inside a transaction that sets `app.tenant_id` (B-R4/B-R5 FORCE RLS) so a forgotten WHERE
// can never leak another tenant. The link-graph is mirrored from the markdown body by the write-gate;
// this repo is the pure SQL substrate the lint scanners run over (no markdown reads, pitfall #12).
use linksan::ParsedLink;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PageRow {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub r#type: String,
    pub reviewed: bool,
    pub char_len: i32,
    pub body_ref: String,
}

/// System-set page mirror fields; char_len/created/updated are set by the caller (the write-gate),
/// never by an LLM (B-R7, pitfall #12).
pub struct PageUpsert {
    pub r#type: String,
    pub slug: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub summary: String,
    pub char_len: i32,
    pub body_ref: String,
    pub source_ids: Vec<Uuid>,
    /// SYNC-D6: sha-256 hex of the sanitized body (set by the write-gate). A warm sync client skips
    /// re-downloading a body whose hash it already holds.
    pub content_hash: String,
}

// `type` is the `wiki_page_type` PG enum; cast to text so it decodes into `PageRow.r#type: String`.
const PAGE_COLS: &str = "id, slug, title, aliases, tags, type::text AS \"type\", reviewed, char_len, body_ref";

pub struct WikiRepo {
    pool: PgPool,
}
impl WikiRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// A clone of the underlying pool — the bounded-concurrency ingest stage spawns per-item tasks that
    /// each construct their own short-lived gate/repo over this shared pool.
    pub fn pool(&self) -> PgPool {
        self.pool.clone()
    }

    /// S2-R26/R5 — catalog rows for index.md, read from PG (slug,title,summary,aliases) — never a body
    /// read (pitfall #12). Runs inside a tenant-GUC tx so FORCE RLS scopes it.
    pub async fn catalog_rows(
        &self,
        tenant: Uuid,
    ) -> sqlx::Result<Vec<(String, String, String, Vec<String>)>> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let rows = sqlx::query_as::<_, (String, String, String, Vec<String>)>(
            "SELECT slug, title, summary, aliases FROM wiki_pages \
             WHERE tenant_id=$1 AND deleted_at IS NULL AND type IN ('entity','concept','source') \
             ORDER BY updated DESC",
        )
        .bind(tenant)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows)
    }

    /// S2-R28 — resolve a non-deleted page's `body_ref` (the absolute vault path) by slug for the
    /// query engine to load. Pure SQL (no file IO — `store` never touches the markdown); the content
    /// caller (the query engine, which holds the vault root) reads the file. None ⇒ no such page.
    pub async fn body_ref_by_slug(&self, tenant: Uuid, slug: &str) -> sqlx::Result<Option<String>> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT body_ref FROM wiki_pages \
             WHERE tenant_id=$1 AND deleted_at IS NULL AND (slug=$2 OR $2 = ANY(aliases)) LIMIT 1",
        )
        .bind(tenant)
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row.map(|r| r.0))
    }

    /// S2-R21 — existing-pages lookup is a tenant-scoped SQL query, never a vault scan.
    pub async fn existing_pages(&self, tenant: Uuid) -> sqlx::Result<Vec<PageRow>> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let rows = sqlx::query_as::<_, PageRow>(&format!(
            "SELECT {PAGE_COLS} FROM wiki_pages WHERE tenant_id=$1 AND deleted_at IS NULL"
        ))
        .bind(tenant)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows)
    }

    /// Raw (no app-level tenant WHERE) — proves RLS still filters (test belt). The GUC is still set, so
    /// only the current tenant's rows are visible even though the SQL omits the tenant predicate.
    pub async fn all_pages_raw(&self, tenant: Uuid) -> sqlx::Result<Vec<PageRow>> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let rows = sqlx::query_as::<_, PageRow>(&format!(
            "SELECT {PAGE_COLS} FROM wiki_pages WHERE deleted_at IS NULL"
        ))
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows)
    }

    /// Upsert a page mirror row; char_len/created/updated are system-set by the caller (write-gate).
    pub async fn upsert_page(&self, tenant: Uuid, p: &PageUpsert) -> sqlx::Result<Uuid> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        // SYNC-D6/D2: content_hash (sha-256 of the sanitized body) + sync_version (a monotonic version
        // bumped on each materialized body write — the base_version conflict precondition). A fresh
        // INSERT starts at 1; an UPDATE bumps the existing version by 1.
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO wiki_pages (tenant_id, type, slug, title, aliases, tags, summary, char_len, body_ref, source_ids, content_hash, sync_version, updated) \
             VALUES ($1,$2::wiki_page_type,$3,$4,$5,$6,$7,$8,$9,$10,$11, 1, now()) \
             ON CONFLICT (tenant_id, type, slug) WHERE deleted_at IS NULL DO UPDATE \
             SET title=EXCLUDED.title, aliases=EXCLUDED.aliases, tags=EXCLUDED.tags, summary=EXCLUDED.summary, \
                 char_len=EXCLUDED.char_len, content_hash=EXCLUDED.content_hash, \
                 source_ids=ARRAY(SELECT DISTINCT unnest(wiki_pages.source_ids || EXCLUDED.source_ids)), \
                 sync_version=wiki_pages.sync_version+1, updated=now() \
             RETURNING id",
        )
        .bind(tenant)
        .bind(&p.r#type)
        .bind(&p.slug)
        .bind(&p.title)
        .bind(&p.aliases)
        .bind(&p.tags)
        .bind(&p.summary)
        .bind(p.char_len)
        .bind(&p.body_ref)
        .bind(&p.source_ids)
        .bind(&p.content_hash)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row.0)
    }

    /// Replace the outgoing link-graph edges for src_page_id (delete + re-insert), resolving targets by
    /// slug/alias. A target that resolves to nothing leaves `target_page_id` NULL ⇒ a dead link.
    pub async fn replace_links(&self, tenant: Uuid, src: Uuid, links: &[ParsedLink]) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        sqlx::query("DELETE FROM wiki_links WHERE tenant_id=$1 AND src_page_id=$2")
            .bind(tenant)
            .bind(src)
            .execute(&mut *tx)
            .await?;
        for l in links {
            sqlx::query(
                "INSERT INTO wiki_links (tenant_id, src_page_id, target_slug, target_type, target_page_id, display) \
                 VALUES ($1,$2,$3,$4::wiki_page_type, \
                   (SELECT id FROM wiki_pages WHERE tenant_id=$1 AND deleted_at IS NULL \
                      AND (slug=$3 OR $3 = ANY(aliases)) LIMIT 1), $5) \
                 ON CONFLICT (tenant_id, src_page_id, target_slug) DO NOTHING",
            )
            .bind(tenant)
            .bind(src)
            .bind(&l.target_slug)
            .bind(&l.target_type)
            .bind(&l.display)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn page(&self, tenant: Uuid, id: Uuid) -> sqlx::Result<PageRow> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let row = sqlx::query_as::<_, PageRow>(&format!(
            "SELECT {PAGE_COLS} FROM wiki_pages WHERE tenant_id=$1 AND id=$2"
        ))
        .bind(tenant)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn links_of(&self, tenant: Uuid, src: Uuid) -> sqlx::Result<Vec<(String, Option<Uuid>)>> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let rows = sqlx::query_as::<_, (String, Option<Uuid>)>(
            "SELECT target_slug, target_page_id FROM wiki_links WHERE tenant_id=$1 AND src_page_id=$2",
        )
        .bind(tenant)
        .bind(src)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows)
    }

    /// Soft-delete a page (reversible Dream merge; B-R9, pitfall #18). Never hard-deletes content.
    pub async fn soft_delete(&self, tenant: Uuid, id: Uuid) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        sqlx::query("UPDATE wiki_pages SET deleted_at=now() WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL")
            .bind(tenant)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    // ── lint scanner helpers: thin tenant-scoped `SELECT id …` runners (pure SQL, no body reads) ──

    /// Run a `SELECT <uuid-col> …` whose only bind is $1 = tenant.
    pub async fn scan_ids(&self, tenant: Uuid, sql: &str) -> sqlx::Result<Vec<Uuid>> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let rows = sqlx::query_as::<_, (Uuid,)>(sql).bind(tenant).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// Run a `SELECT id …` with $1 = tenant and $2 = an i32 bind (e.g. char_len threshold).
    pub async fn scan_ids_bind(&self, tenant: Uuid, sql: &str, n: i32) -> sqlx::Result<Vec<Uuid>> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let rows = sqlx::query_as::<_, (Uuid,)>(sql).bind(tenant).bind(n).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// Run a `SELECT id …` with $1 = tenant and $2 = a text[] vocabulary bind (tag-violation scan).
    pub async fn scan_ids_tags(&self, tenant: Uuid, sql: &str, vocab: &[String]) -> sqlx::Result<Vec<Uuid>> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let rows = sqlx::query_as::<_, (Uuid,)>(sql).bind(tenant).bind(vocab).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// SBX-R5 — the tenant's current vault footprint: (page_count, summed_char_len). RLS-scoped so
    /// FORCE RLS ensures only this tenant's rows are visible even without a WHERE clause.
    pub async fn vault_usage(&self, tenant: Uuid) -> sqlx::Result<(i64, i64)> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let row: (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*)::int8, COALESCE(SUM(char_len),0)::int8 FROM wiki_pages \
             WHERE deleted_at IS NULL",
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row)
    }
}

/// Set the request tenant GUC inside the transaction (`SET LOCAL app.tenant_id`) so FORCE RLS scopes
/// every statement in the tx to this tenant (B-R5). `set_config(..., true)` is the transaction-local
/// form (it reverts at COMMIT/ROLLBACK).
async fn set_tenant(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, tenant: Uuid) -> sqlx::Result<()> {
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
