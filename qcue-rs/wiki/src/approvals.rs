// QCue D13 / A-R19 — the candidates→confirm→canonical gate over the `approvals` table (one gate, one
// table; App. B §4.19). A Dream- or lint-proposed DESTRUCTIVE op (a page merge or delete) is NEVER
// applied to canonical directly: it lands as an `approvals` row (`action ∈ {wiki_merge, wiki_delete}`,
// `status='pending'`, `requested_by`), and the canonical mutation only happens through a separate
// confirm endpoint. The destructive part is reversible (soft-delete; the row carries the rollback
// subject). "candidate" is the conceptual D13 name; the `approvals` row IS the storage (no separate
// candidates table, no requires_confirmation column).
//
// Low-risk edits auto-apply (no approval row); merges and deletes require confirm (pitfall #18). Every
// statement runs inside a per-tx `app.tenant_id` GUC (FORCE RLS).
use serde_json::json;
use sqlx::PgPool;
use store::wiki_repo::WikiRepo;
use uuid::Uuid;

/// A proposed destructive op. The merge folds `from` into `into`; the delete soft-deletes `page`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestructiveOp {
    /// Merge page `from` into page `into` (the dup-merge the lint dedup + Dream both produce).
    WikiMerge { from: Uuid, into: Uuid },
    /// Soft-delete page `page` (a contradicted/superseded page).
    WikiDelete { page: Uuid },
}

impl DestructiveOp {
    fn action(&self) -> &'static str {
        match self {
            DestructiveOp::WikiMerge { .. } => "wiki_merge",
            DestructiveOp::WikiDelete { .. } => "wiki_delete",
        }
    }
    fn subject(&self) -> serde_json::Value {
        match self {
            DestructiveOp::WikiMerge { from, into } => {
                json!({ "from": from.to_string(), "into": into.to_string() })
            }
            DestructiveOp::WikiDelete { page } => json!({ "page": page.to_string() }),
        }
    }
    /// The page that is soft-deleted by this op (the reversible side; the canonical row is untouched
    /// until the confirm endpoint promotes the approval).
    fn soft_delete_target(&self) -> Uuid {
        match self {
            DestructiveOp::WikiMerge { from, .. } => *from,
            DestructiveOp::WikiDelete { page } => *page,
        }
    }
}

/// Route a destructive op through the candidates→confirm gate: insert a PENDING `approvals` row and
/// soft-delete the affected page (reversible). Canonical is otherwise unchanged until a confirm
/// endpoint promotes the approval. `requested_by` ∈ {dream, ingest, lint, user}. Returns the new
/// approval row id.
pub async fn route_destructive(
    pool: &PgPool,
    tenant: Uuid,
    user: Uuid,
    requested_by: &str,
    op: DestructiveOp,
) -> anyhow::Result<Uuid> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await?;
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO approvals (tenant_id, user_id, action, subject_ref, requested_by) \
         VALUES ($1,$2,$3,$4,$5) RETURNING id",
    )
    .bind(tenant)
    .bind(user)
    .bind(op.action())
    .bind(op.subject())
    .bind(requested_by)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    // The reversible destructive side: soft-delete the affected page (the merge source / the deleted
    // page). The write-gate's `WikiRepo::soft_delete` stamps `deleted_at` and runs under the GUC.
    let repo = WikiRepo::new(pool.clone());
    repo.soft_delete(tenant, op.soft_delete_target()).await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn op_action_and_subject_are_stable() {
        let from = Uuid::now_v7();
        let into = Uuid::now_v7();
        let m = DestructiveOp::WikiMerge { from, into };
        assert_eq!(m.action(), "wiki_merge");
        assert_eq!(m.soft_delete_target(), from);
        assert_eq!(m.subject()["from"], from.to_string());
        let d = DestructiveOp::WikiDelete { page: from };
        assert_eq!(d.action(), "wiki_delete");
        assert_eq!(d.soft_delete_target(), from);
    }
}
