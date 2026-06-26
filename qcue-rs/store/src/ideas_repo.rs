// QCue S2-R7/A-R10 — ideas insert/state + captures_since count for the Dream session gate. Every op is
// tenant-scoped via a per-tx `app.tenant_id` GUC (B-R4/B-R5 FORCE RLS). The Dream agent itself is the
// next milestone; `captures_since` is the LLM-free authoritative session-gate count it will read.
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

pub struct IdeasRepo {
    pool: PgPool,
}
impl IdeasRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// A-R10 — authoritative live COUNT of new captures since the clock, excluding the current session.
    pub async fn captures_since(
        &self,
        tenant: Uuid,
        since: DateTime<Utc>,
        current_session: Uuid,
    ) -> sqlx::Result<i64> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let row: (i64,) = sqlx::query_as(
            "SELECT count(*) FROM ideas WHERE tenant_id=$1 AND captured_at > $2 \
             AND (ingest_job_id IS NULL OR ingest_job_id <> $3) AND active",
        )
        .bind(tenant)
        .bind(since)
        .bind(current_session)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row.0)
    }

    /// S2-R7 — transition ingest_state (e.g. 'skipped_redundant').
    pub async fn set_ingest_state(&self, tenant: Uuid, idea_id: Uuid, state: &str) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        sqlx::query("UPDATE ideas SET ingest_state=$3::ingest_state WHERE tenant_id=$1 AND id=$2")
            .bind(tenant)
            .bind(idea_id)
            .bind(state)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// DIG-R1 — stamp last_ingested_at=now() (called by IngestJob::run on the 'ingested' transition).
    /// Idempotent: re-running the digest on an unchanged idea simply re-stamps the same instant.
    pub async fn set_last_ingested(&self, tenant: Uuid, idea_id: Uuid) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        sqlx::query("UPDATE ideas SET last_ingested_at=now() WHERE tenant_id=$1 AND id=$2")
            .bind(tenant)
            .bind(idea_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// DIG-R2 — the incremental-digest dirty scan: active captures that are 'pending' OR were edited
    /// since their last ingest (updated_at > last_ingested_at). Ordered by captured_at so the oldest
    /// dirty capture is enqueued first. Tenant-scoped by the GUC (FORCE RLS).
    pub async fn select_dirty_for_ingest(&self, tenant: Uuid) -> sqlx::Result<Vec<Uuid>> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM ideas \
             WHERE tenant_id=$1 AND active \
               AND (ingest_state='pending' \
                    OR (last_ingested_at IS NOT NULL AND updated_at > last_ingested_at)) \
             ORDER BY captured_at",
        )
        .bind(tenant)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// DIG-R4 — the slug of a non-deleted SOURCE page previously produced from this idea (resolved via
    /// the `source_ids` provenance array, GIN index `wiki_pages_sources_gin`). None ⇒ no prior page, so
    /// the ingest derives a fresh slug. Reusing the prior slug makes the re-ingest update the SAME page
    /// in place (ON CONFLICT (tenant,type,slug)) rather than minting a new slug and orphaning the old.
    pub async fn source_page_slug(&self, tenant: Uuid, idea_id: Uuid) -> sqlx::Result<Option<String>> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT slug FROM wiki_pages \
             WHERE tenant_id=$1 AND type='source' AND deleted_at IS NULL AND source_ids @> ARRAY[$2]::uuid[] \
             ORDER BY updated DESC LIMIT 1",
        )
        .bind(tenant)
        .bind(idea_id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row.map(|r| r.0))
    }
}

async fn set_tenant(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, tenant: Uuid) -> sqlx::Result<()> {
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
