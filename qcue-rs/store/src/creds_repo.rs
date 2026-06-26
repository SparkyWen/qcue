// QCue S1-R34 — provider_credentials as the source of truth (Appendix B §4.6). RLS GUC per tx.
use std::sync::Mutex;

use sqlx::PgPool;
use uuid::Uuid;

pub struct CredRow {
    pub id: Uuid,
    pub provider: String,
    pub status: String,
    pub key_hint: String,
}

pub struct CredsRepo {
    pool: PgPool,
    // The active request tenant. `set_tenant_guc` records it; every method opens a transaction and
    // applies `SET LOCAL app.tenant_id` on THAT connection before its DML, so RLS (B-R4/B-R5) sees
    // the right tenant. This is required against a real pooled connection — a session-level
    // `set_config(...,false)` only affects one pooled connection and would not carry to the next.
    tenant: Mutex<Option<Uuid>>,
}
impl CredsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, tenant: Mutex::new(None) }
    }

    /// Set the RLS GUC for subsequent operations (B-R5). Real requests use SET LOCAL inside the
    /// request tx; here we remember the tenant and re-apply it per transaction (pooled-conn safe).
    pub async fn set_tenant_guc(&self, tenant: Uuid) -> Result<(), sqlx::Error> {
        if let Ok(mut g) = self.tenant.lock() {
            *g = Some(tenant);
        }
        Ok(())
    }

    fn current_tenant(&self) -> Result<Uuid, sqlx::Error> {
        self.tenant
            .lock()
            .ok()
            .and_then(|g| *g)
            .ok_or_else(|| sqlx::Error::Configuration("tenant GUC not set".into()))
    }

    /// Insert a credential with placeholder ciphertext (real ciphertext supplied by S3's vault).
    ///
    /// NOTE (deviation from plan): the Appendix B schema enforces a hard
    /// `provider_credentials.tenant_id REFERENCES tenants(id)` FK, so the test helper first seeds
    /// the parent `tenants` row idempotently (the plan's INSERT alone would violate the FK against
    /// the verbatim DDL). The seed satisfies the `tenants` NOT NULL columns (`namespace`).
    pub async fn insert_test_credential(
        &self,
        tenant: Uuid,
        provider: &str,
        hint: &str,
    ) -> Result<Uuid, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        // `tenants` is the global root (no RLS); seed it idempotently to satisfy the FK.
        sqlx::query(
            "INSERT INTO tenants (id, slug, display_name, namespace)
             VALUES ($1, $2, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(tenant)
        .bind(tenant.to_string())
        .bind(format!("t/{tenant}"))
        .execute(&mut *tx)
        .await?;
        let rec = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO provider_credentials
               (tenant_id, provider, key_ciphertext, key_nonce, key_tag, dek_wrapped, kek_id, key_hint)
             VALUES ($1,$2,'\\x00','\\x00','\\x00','\\x00','kek-v1',$3) RETURNING id",
        )
        .bind(tenant)
        .bind(provider)
        .bind(hint)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rec)
    }

    /// S1-R32/R34 — flip to exhausted with a 1h cooldown (Postgres source-of-truth; Redis holds the live timer).
    pub async fn mark_exhausted(&self, id: Uuid, _provider: &str) -> Result<(), sqlx::Error> {
        let tenant = self.current_tenant()?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE provider_credentials SET status='exhausted', cooldown_until = now() + interval '1 hour' WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// S1-R31/R34 — re-read the row before declaring dead (another worker may have refreshed it).
    pub async fn mark_dead(&self, id: Uuid) -> Result<(), sqlx::Error> {
        let tenant = self.current_tenant()?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE provider_credentials SET status='dead', dead_at = now() WHERE id=$1 AND status<>'dead'")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list(&self, tenant: Uuid, provider: &str) -> Result<Vec<CredRow>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        let rows = sqlx::query_as::<_, (Uuid, String, String, String)>(
            "SELECT id, provider, status::text, key_hint FROM provider_credentials WHERE tenant_id=$1 AND provider=$2",
        )
        .bind(tenant)
        .bind(provider)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|(id, provider, status, key_hint)| CredRow { id, provider, status, key_hint })
            .collect())
    }
}
