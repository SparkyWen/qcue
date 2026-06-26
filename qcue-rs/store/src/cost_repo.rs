// QCue S1-R55 (consumes B-R20) — cost_ledger pre-call read + accrue against the daily ceiling (D17).
//
// Two row granularities live in one table (Appendix B §4.18), distinguished by `scope`: a
// per-tenant/day row (`scope='tenant'`, `user_id IS NULL`) and a per-user/day row (`scope='user'`).
// The controller reads BOTH `cost_micros` totals against `tenants.daily_cost_cap_micros` /
// `users.per_user_daily_cost_cap_micros` BEFORE dispatching to a provider/STT; over-ceiling → refusal,
// no call made (B-R20). Every method opens a transaction and applies `SET LOCAL app.tenant_id` on
// THAT connection before its DML so RLS (B-R4/B-R5) is enforced per the established repo pattern.
use sqlx::PgPool;
use uuid::Uuid;

/// The five CanonicalUsage token kinds accrued into the ledger (all real ledger columns, B-R20).
#[derive(Clone, Copy, Debug, Default)]
pub struct CostUsage {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub reasoning: i64,
}

pub struct CostRepo {
    pool: PgPool,
}

impl CostRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The underlying pool (a clone). Lets callers route sibling tenant-scoped statements (e.g. the
    /// candidates→confirm approvals gate) over the same pool without re-threading a connection.
    pub fn pool(&self) -> PgPool {
        self.pool.clone()
    }

    /// Idempotently ensure the parent `tenants`/`users` rows exist (hard FKs in §4.18) inside the
    /// tenant tx, WITHOUT touching their daily-cap columns (so a prior `seed_caps` is preserved).
    async fn ensure_parents(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        tenant: Uuid,
        user: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO tenants (id, slug, display_name, namespace)
             VALUES ($1, $2, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(tenant)
        .bind(tenant.to_string())
        .bind(format!("t/{tenant}"))
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "INSERT INTO users (id, tenant_id, email)
             VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user)
        .bind(tenant)
        .bind(format!("{user}@seed.qcue"))
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    /// Idempotently ensure ONLY the parent `tenants` row exists (the tenant-scope ledger row's FK),
    /// without a `users` row — for the per-turn chokepoint accrual that has no originating user_id.
    async fn ensure_tenant(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        tenant: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO tenants (id, slug, display_name, namespace)
             VALUES ($1, $2, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(tenant)
        .bind(tenant.to_string())
        .bind(format!("t/{tenant}"))
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    /// Accrue spend into ONLY the tenant-scope day row (`scope='tenant'`, `user_id IS NULL`) — the row
    /// `/v1/cost/today` and `/v1/cost/ledger` read. Used by the per-turn provider chokepoint where the
    /// originating user isn't threaded to the model seam. UPDATE-first / INSERT-if-absent, tenant-isolated.
    pub async fn accrue_tenant(
        &self,
        tenant: Uuid,
        usage: CostUsage,
        cost_micros: i64,
        provider: &str,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        Self::ensure_tenant(&mut tx, tenant).await?;
        let updated = sqlx::query(
            "UPDATE cost_ledger SET
               input_tokens       = input_tokens + $2,
               output_tokens      = output_tokens + $3,
               cache_read_tokens  = cache_read_tokens + $4,
               cache_write_tokens = cache_write_tokens + $5,
               reasoning_tokens   = reasoning_tokens + $6,
               cost_micros        = cost_micros + $7
             WHERE tenant_id=$1 AND scope='tenant' AND user_id IS NULL AND day=current_date",
        )
        .bind(tenant)
        .bind(usage.input)
        .bind(usage.output)
        .bind(usage.cache_read)
        .bind(usage.cache_write)
        .bind(usage.reasoning)
        .bind(cost_micros)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            let breakdown = serde_json::json!({ provider: cost_micros });
            sqlx::query(
                "INSERT INTO cost_ledger
                   (tenant_id, scope, user_id, day, input_tokens, output_tokens,
                    cache_read_tokens, cache_write_tokens, reasoning_tokens, cost_micros, provider_breakdown)
                 VALUES ($1,'tenant',NULL,current_date,$2,$3,$4,$5,$6,$7,$8)",
            )
            .bind(tenant)
            .bind(usage.input)
            .bind(usage.output)
            .bind(usage.cache_read)
            .bind(usage.cache_write)
            .bind(usage.reasoning)
            .bind(cost_micros)
            .bind(&breakdown)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Test/bootstrap helper: seed the parent rows with explicit daily caps (no spend accrued).
    pub async fn seed_caps(
        &self,
        tenant: Uuid,
        user: Uuid,
        tenant_cap: i64,
        user_cap: i64,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        sqlx::query(
            "INSERT INTO tenants (id, slug, display_name, namespace, daily_cost_cap_micros)
             VALUES ($1, $2, $2, $3, $4)
             ON CONFLICT (id) DO UPDATE SET daily_cost_cap_micros = EXCLUDED.daily_cost_cap_micros",
        )
        .bind(tenant)
        .bind(tenant.to_string())
        .bind(format!("t/{tenant}"))
        .bind(tenant_cap)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO users (id, tenant_id, email, per_user_daily_cost_cap_micros)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO UPDATE SET per_user_daily_cost_cap_micros = EXCLUDED.per_user_daily_cost_cap_micros",
        )
        .bind(user)
        .bind(tenant)
        .bind(format!("{user}@seed.qcue"))
        .bind(user_cap)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// B-R20 — read today's accrued `cost_micros` for the tenant-scope row and the user-scope row.
    /// A day with no ledger row reads as 0 (fresh-day default). Returns (tenant_micros, user_micros).
    pub async fn read_today(&self, tenant: Uuid, user: Uuid) -> Result<(i64, i64), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let t: Option<i64> = sqlx::query_scalar(
            "SELECT cost_micros FROM cost_ledger
             WHERE tenant_id=$1 AND scope='tenant' AND user_id IS NULL AND day=current_date",
        )
        .bind(tenant)
        .fetch_optional(&mut *tx)
        .await?;
        let u: Option<i64> = sqlx::query_scalar(
            "SELECT cost_micros FROM cost_ledger
             WHERE tenant_id=$1 AND scope='user' AND user_id=$2 AND day=current_date",
        )
        .bind(tenant)
        .bind(user)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((t.unwrap_or(0), u.unwrap_or(0)))
    }

    /// Accrue spend into BOTH the tenant-scope and user-scope day rows (Appendix B §4.18 upsert).
    ///
    /// NOTE: the verbatim `UNIQUE (tenant_id, scope, user_id, day)` uses default NULLS-DISTINCT, so a
    /// tenant-scope row (`user_id IS NULL`) cannot be reached by `ON CONFLICT (... user_id ...)`. The
    /// tenant row is therefore upserted UPDATE-first / INSERT-if-absent (single tx, tenant-isolated);
    /// the user row (non-NULL `user_id`) uses the appendix's `ON CONFLICT` upsert directly.
    pub async fn accrue(
        &self,
        tenant: Uuid,
        user: Uuid,
        usage: CostUsage,
        cost_micros: i64,
        provider: &str,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        Self::ensure_parents(&mut tx, tenant, user).await?;
        let breakdown = serde_json::json!({ provider: cost_micros });

        // tenant-scope row (user_id IS NULL): UPDATE-first then INSERT-if-absent.
        let updated = sqlx::query(
            "UPDATE cost_ledger SET
               input_tokens       = input_tokens + $2,
               output_tokens      = output_tokens + $3,
               cache_read_tokens  = cache_read_tokens + $4,
               cache_write_tokens = cache_write_tokens + $5,
               reasoning_tokens   = reasoning_tokens + $6,
               cost_micros        = cost_micros + $7
             WHERE tenant_id=$1 AND scope='tenant' AND user_id IS NULL AND day=current_date",
        )
        .bind(tenant)
        .bind(usage.input)
        .bind(usage.output)
        .bind(usage.cache_read)
        .bind(usage.cache_write)
        .bind(usage.reasoning)
        .bind(cost_micros)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            sqlx::query(
                "INSERT INTO cost_ledger
                   (tenant_id, scope, user_id, day, input_tokens, output_tokens,
                    cache_read_tokens, cache_write_tokens, reasoning_tokens, cost_micros, provider_breakdown)
                 VALUES ($1,'tenant',NULL,current_date,$2,$3,$4,$5,$6,$7,$8)",
            )
            .bind(tenant)
            .bind(usage.input)
            .bind(usage.output)
            .bind(usage.cache_read)
            .bind(usage.cache_write)
            .bind(usage.reasoning)
            .bind(cost_micros)
            .bind(&breakdown)
            .execute(&mut *tx)
            .await?;
        }

        // user-scope row (user_id NOT NULL): appendix ON CONFLICT upsert.
        sqlx::query(
            "INSERT INTO cost_ledger
               (tenant_id, scope, user_id, day, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, reasoning_tokens, cost_micros, provider_breakdown)
             VALUES ($1,'user',$2,current_date,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT (tenant_id, scope, user_id, day) DO UPDATE SET
               input_tokens       = cost_ledger.input_tokens + EXCLUDED.input_tokens,
               output_tokens      = cost_ledger.output_tokens + EXCLUDED.output_tokens,
               cache_read_tokens  = cost_ledger.cache_read_tokens + EXCLUDED.cache_read_tokens,
               cache_write_tokens = cost_ledger.cache_write_tokens + EXCLUDED.cache_write_tokens,
               reasoning_tokens   = cost_ledger.reasoning_tokens + EXCLUDED.reasoning_tokens,
               cost_micros        = cost_ledger.cost_micros + EXCLUDED.cost_micros,
               updated_at         = now()",
        )
        .bind(tenant)
        .bind(user)
        .bind(usage.input)
        .bind(usage.output)
        .bind(usage.cache_read)
        .bind(usage.cache_write)
        .bind(usage.reasoning)
        .bind(cost_micros)
        .bind(&breakdown)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// B-R20 (D17) — the pre-call ceiling check: read today's tenant+user spend and compare to the
    /// configured caps. `Ok(Ok(()))` = under both ceilings (call allowed); `Ok(Err(reason))` =
    /// over-ceiling refusal (NO provider call may be made). The outer `Result` is DB failure only.
    #[allow(clippy::type_complexity)]
    pub async fn check_ceiling(
        &self,
        tenant: Uuid,
        user: Uuid,
    ) -> Result<Result<(), String>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, tenant).await?;
        let tenant_cap: Option<i64> =
            sqlx::query_scalar("SELECT daily_cost_cap_micros FROM tenants WHERE id=$1")
                .bind(tenant)
                .fetch_optional(&mut *tx)
                .await?;
        let user_cap: Option<i64> =
            sqlx::query_scalar("SELECT per_user_daily_cost_cap_micros FROM users WHERE id=$1")
                .bind(user)
                .fetch_optional(&mut *tx)
                .await?;
        let t_spent: i64 = sqlx::query_scalar(
            "SELECT coalesce(sum(cost_micros),0)::bigint FROM cost_ledger
             WHERE tenant_id=$1 AND scope='tenant' AND user_id IS NULL AND day=current_date",
        )
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await?;
        let u_spent: i64 = sqlx::query_scalar(
            "SELECT coalesce(sum(cost_micros),0)::bigint FROM cost_ledger
             WHERE tenant_id=$1 AND scope='user' AND user_id=$2 AND day=current_date",
        )
        .bind(tenant)
        .bind(user)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;

        if let Some(cap) = tenant_cap
            && t_spent >= cap
        {
            return Ok(Err(format!(
                "tenant daily cost ceiling reached: {t_spent} >= {cap} micros"
            )));
        }
        if let Some(cap) = user_cap
            && u_spent >= cap
        {
            return Ok(Err(format!(
                "user daily cost ceiling reached: {u_spent} >= {cap} micros"
            )));
        }
        Ok(Ok(()))
    }
}

/// Apply the per-transaction RLS GUC (`SET LOCAL app.tenant_id`) on the tx's connection (B-R4/B-R5).
async fn set_tenant(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
