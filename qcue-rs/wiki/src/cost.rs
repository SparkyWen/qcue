// QCue S2-R64/R19 — pre-call ceiling + per-request accrual. Delegates the ledger SQL to the
// established `store::cost_repo::CostRepo` (which owns the per-tx RLS GUC + the verbatim §4.18 upsert
// that correctly handles the NULL-user_id tenant row), and layers the per-request dedup on top so the
// single per-call 5-field CanonicalUsage is never summed across content blocks (S2-R19).
use protocol::CanonicalUsage;
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::Mutex;
use store::cost_repo::{CostRepo, CostUsage};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
#[error("cost ceiling reached: {0}")]
pub struct CeilingReached(pub String);

pub struct CostGuard {
    repo: CostRepo,
    seen_requests: Mutex<HashSet<String>>,
}

impl CostGuard {
    pub fn new(pool: PgPool) -> Self {
        Self { repo: CostRepo::new(pool), seen_requests: Mutex::new(HashSet::new()) }
    }

    /// The underlying pool (a clone) — lets the Dream agent route its candidates→confirm approvals
    /// inserts over the same tenant-scoped pool the cost ledger uses (A-R19).
    pub fn pool(&self) -> PgPool {
        self.repo.pool()
    }

    /// D17 — read tenant + user day spend against the caps BEFORE dispatching. Over → CeilingReached
    /// (no call). The DB error path surfaces as an `anyhow::Error` so ingest aborts cleanly.
    pub async fn check_before_call(&self, tenant: Uuid, user: Uuid) -> anyhow::Result<()> {
        match self.repo.check_ceiling(tenant, user).await? {
            Ok(()) => Ok(()),
            Err(reason) => Err(CeilingReached(reason).into()),
        }
    }

    /// S2-R19 — accrue the single per-call usage once per request_id (never summed across content
    /// blocks). A repeated request_id is a no-op (idempotent re-delivery / double-count guard).
    pub async fn accrue(
        &self,
        tenant: Uuid,
        user: Uuid,
        request_id: &str,
        u: &CanonicalUsage,
        cost_micros: i64,
    ) -> anyhow::Result<()> {
        {
            let mut seen = self
                .seen_requests
                .lock()
                .map_err(|_| anyhow::anyhow!("cost guard mutex poisoned"))?;
            if !seen.insert(request_id.to_string()) {
                return Ok(()); // dedup — same request, no double-bill
            }
        }
        let usage = CostUsage {
            input: u.input as i64,
            output: u.output as i64,
            cache_read: u.cache_read as i64,
            cache_write: u.cache_write as i64,
            reasoning: u.reasoning as i64,
        };
        self.repo.accrue(tenant, user, usage, cost_micros, "wiki").await?;
        Ok(())
    }
}
