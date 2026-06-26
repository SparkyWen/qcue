// QCue A-R4..R8 / B-R19 — `wiki_consolidation` lock-as-clock (lease + clock in one row), the faithful
// port of Claude Code 2.1.88's `.consolidate-lock` (file mtime = lastConsolidatedAt, body = PID,
// HOLDER_STALE_MS=1h, dead-PID reclaim, two-writer last-wins) — App. A §2.2.
//
// THE CLOCK is `last_consolidated_at`: the acquire WRITES it (the row analog of writing the PID which
// advances mtime as a side effect — there is no separate "stamp" step). A live unexpired lease blocks
// a second worker (Postgres' `WHERE` predicate sees the winner's fresh `lease_expires`, the row analog
// of Claude's read-after-write PID verification). `rollback(prior)` REWINDS the clock so the time-gate
// fires again — the scan-throttle is the backoff. `release` frees the lease but keeps the advanced
// clock (a successful dream pushes `last_consolidated_at` forward by minHours of gating).
//
// Every statement runs inside a per-tx `app.tenant_id` GUC (FORCE RLS bites even the table owner).
use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// The lock-as-clock seam. One row per tenant; lease + clock in one inode-equivalent.
#[async_trait]
pub trait ConsolidationLock: Send + Sync {
    /// = `readLastConsolidatedAt` — the time-gate input; an absent/NULL clock reads epoch 0.
    async fn read_clock(&self, t: Uuid) -> anyhow::Result<DateTime<Utc>>;
    /// = `tryAcquireConsolidationLock` — advances the clock & writes the lease IF free or stale; returns
    /// `Some(prior_clock)` (for rollback) or `None` when a live unexpired holder blocks it.
    async fn try_acquire(&self, t: Uuid, holder: &str) -> anyhow::Result<Option<DateTime<Utc>>>;
    /// A-R7 — release frees the lease but keeps the advanced clock (success path).
    async fn release(&self, t: Uuid) -> anyhow::Result<()>;
    /// A-R8 / B-R19 — rollback rewinds the clock to `prior`, frees the lease (failure/kill path).
    async fn rollback(&self, t: Uuid, prior: DateTime<Utc>) -> anyhow::Result<()>;
}

/// The Postgres lock-as-clock over the `wiki_consolidation` row.
pub struct PgConsolidationLock {
    pool: PgPool,
}
impl PgConsolidationLock {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Begin a tenant-scoped transaction (FORCE RLS requires the GUC be set per write).
    async fn tenant_tx(
        &self,
        t: Uuid,
    ) -> anyhow::Result<sqlx::Transaction<'_, sqlx::Postgres>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(t.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
}

#[async_trait]
impl ConsolidationLock for PgConsolidationLock {
    async fn read_clock(&self, t: Uuid) -> anyhow::Result<DateTime<Utc>> {
        let mut tx = self.tenant_tx(t).await?;
        let row: Option<(Option<DateTime<Utc>>,)> =
            sqlx::query_as("SELECT last_consolidated_at FROM wiki_consolidation WHERE tenant_id=$1")
                .bind(t)
                .fetch_optional(&mut *tx)
                .await?;
        tx.commit().await?;
        Ok(row
            .and_then(|r| r.0)
            .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().unwrap_or_default()))
    }

    /// A-R5 — atomic acquire (only if free or stale); advances the clock; returns the prior clock, or
    /// None if blocked by a live unexpired holder. The capture-prior + conditional-UPDATE run in ONE
    /// tx so the read and the claim are consistent under contention.
    async fn try_acquire(&self, t: Uuid, holder: &str) -> anyhow::Result<Option<DateTime<Utc>>> {
        let mut tx = self.tenant_tx(t).await?;
        // capture the prior clock first (for rollback). NULL → epoch 0.
        let prior_row: Option<(Option<DateTime<Utc>>,)> =
            sqlx::query_as("SELECT last_consolidated_at FROM wiki_consolidation WHERE tenant_id=$1")
                .bind(t)
                .fetch_optional(&mut *tx)
                .await?;
        let prior = prior_row
            .and_then(|r| r.0)
            .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().unwrap_or_default());
        // claim IFF the lease is free (holder NULL) or stale (lease_expires < now). The UPDATE advances
        // the clock as a side effect — exactly like writing the PID advancing mtime.
        let claimed: Option<(Uuid,)> = sqlx::query_as(
            "UPDATE wiki_consolidation \
                SET holder=$2, lease_expires=now()+interval '1 hour', last_scan_at=now(), \
                    last_consolidated_at=now() \
              WHERE tenant_id=$1 AND (holder IS NULL OR lease_expires IS NULL OR lease_expires < now()) \
              RETURNING tenant_id",
        )
        .bind(t)
        .bind(holder)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(claimed.map(|_| prior))
    }

    async fn release(&self, t: Uuid) -> anyhow::Result<()> {
        let mut tx = self.tenant_tx(t).await?;
        sqlx::query(
            "UPDATE wiki_consolidation SET holder=NULL, lease_expires=NULL, sessions_since_last=0 \
             WHERE tenant_id=$1",
        )
        .bind(t)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn rollback(&self, t: Uuid, prior: DateTime<Utc>) -> anyhow::Result<()> {
        let mut tx = self.tenant_tx(t).await?;
        sqlx::query(
            "UPDATE wiki_consolidation \
                SET last_consolidated_at=$2, holder=NULL, lease_expires=NULL, \
                    rollback_count=rollback_count+1 \
              WHERE tenant_id=$1",
        )
        .bind(t)
        .bind(prior)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
