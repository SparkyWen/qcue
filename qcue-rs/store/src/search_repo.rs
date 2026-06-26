// QCue A-R21 — execute the routed query per `SearchMode`, tenant-scoped (RLS). Spans the three sources
// recall searches: `ideas` (captures), `messages` (transcript), `wiki_pages` (the wiki). The model
// authored the pattern; `search_route::route_search` picked the index path; this executor only runs the
// matching SQL. Every method runs inside a per-tx `app.tenant_id` GUC so FORCE RLS (B-R4/B-R5) scopes
// it — a forgotten WHERE can never leak another tenant's rows (pitfall #14).
use search_route::SearchMode;
use sqlx::PgPool;
use uuid::Uuid;

/// One un-bookended hit. `session_id` lets the tool exclude the current session + collapse a lineage;
/// `source_kind` ('idea'|'message'|'wiki') tags where the row came from.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RawHit {
    pub id: Uuid,
    pub body: String,
    pub session_id: Option<Uuid>,
    pub source_kind: String,
}

pub struct SearchRepo {
    pool: PgPool,
}

impl SearchRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Search `ideas.body` per the routed mode. Tsvector → FTS rank; Trigram → similarity; Like → ILIKE.
    /// `unaccent($2)` folds diacritics at query time to match the STORED `search_tsv` column's folding.
    pub async fn search_ideas(
        &self,
        tenant: Uuid,
        q: &str,
        mode: SearchMode,
        limit: i64,
    ) -> sqlx::Result<Vec<RawHit>> {
        let sql = match mode {
            SearchMode::Tsvector => {
                "SELECT id, body, NULL::uuid AS session_id, 'idea' AS source_kind FROM ideas \
                 WHERE tenant_id=$1 AND active AND search_tsv @@ plainto_tsquery('simple', unaccent($2)) \
                 ORDER BY ts_rank(search_tsv, plainto_tsquery('simple', unaccent($2))) DESC LIMIT $3"
            }
            SearchMode::Trigram => {
                "SELECT id, body, NULL::uuid AS session_id, 'idea' AS source_kind FROM ideas \
                 WHERE tenant_id=$1 AND active AND body ILIKE '%'||$2||'%' \
                 ORDER BY similarity(body,$2) DESC LIMIT $3"
            }
            SearchMode::Like => {
                "SELECT id, body, NULL::uuid AS session_id, 'idea' AS source_kind FROM ideas \
                 WHERE tenant_id=$1 AND active AND body ILIKE '%'||$2||'%' LIMIT $3"
            }
        };
        self.run(tenant, sql, q, limit).await
    }

    /// Search `messages.content` (the transcript) per the routed mode (active rows only).
    pub async fn search_messages(
        &self,
        tenant: Uuid,
        q: &str,
        mode: SearchMode,
        limit: i64,
    ) -> sqlx::Result<Vec<RawHit>> {
        let sql = match mode {
            SearchMode::Tsvector => {
                "SELECT id, coalesce(content,'') AS body, session_id, 'message' AS source_kind FROM messages \
                 WHERE tenant_id=$1 AND active AND search_tsv @@ plainto_tsquery('simple', unaccent($2)) \
                 ORDER BY ts_rank(search_tsv, plainto_tsquery('simple', unaccent($2))) DESC LIMIT $3"
            }
            SearchMode::Trigram => {
                "SELECT id, coalesce(content,'') AS body, session_id, 'message' AS source_kind FROM messages \
                 WHERE tenant_id=$1 AND active AND content ILIKE '%'||$2||'%' \
                 ORDER BY similarity(coalesce(content,''),$2) DESC LIMIT $3"
            }
            SearchMode::Like => {
                "SELECT id, coalesce(content,'') AS body, session_id, 'message' AS source_kind FROM messages \
                 WHERE tenant_id=$1 AND active AND content ILIKE '%'||$2||'%' LIMIT $3"
            }
        };
        self.run(tenant, sql, q, limit).await
    }

    /// Search the wiki (`wiki_pages` title/aliases/summary tsvector, body via trgm/like) per the mode.
    pub async fn search_wiki(
        &self,
        tenant: Uuid,
        q: &str,
        mode: SearchMode,
        limit: i64,
    ) -> sqlx::Result<Vec<RawHit>> {
        let sql = match mode {
            SearchMode::Tsvector => {
                "SELECT id, coalesce(summary,'') AS body, NULL::uuid AS session_id, 'wiki' AS source_kind FROM wiki_pages \
                 WHERE tenant_id=$1 AND deleted_at IS NULL AND search_tsv @@ plainto_tsquery('simple', unaccent($2)) \
                 ORDER BY ts_rank(search_tsv, plainto_tsquery('simple', unaccent($2))) DESC LIMIT $3"
            }
            SearchMode::Trigram | SearchMode::Like => {
                "SELECT id, coalesce(summary,'') AS body, NULL::uuid AS session_id, 'wiki' AS source_kind FROM wiki_pages \
                 WHERE tenant_id=$1 AND deleted_at IS NULL \
                   AND (title ILIKE '%'||$2||'%' OR coalesce(summary,'') ILIKE '%'||$2||'%') LIMIT $3"
            }
        };
        self.run(tenant, sql, q, limit).await
    }

    async fn run(&self, tenant: Uuid, sql: &str, q: &str, limit: i64) -> sqlx::Result<Vec<RawHit>> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let rows = sqlx::query_as::<_, RawHit>(sql)
            .bind(tenant)
            .bind(q)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(rows)
    }
}

/// Set the request tenant GUC inside the tx (`SET LOCAL app.tenant_id`) so FORCE RLS scopes every
/// statement to this tenant (B-R5); `set_config(..., true)` reverts at COMMIT/ROLLBACK.
async fn set_tenant(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, tenant: Uuid) -> sqlx::Result<()> {
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
