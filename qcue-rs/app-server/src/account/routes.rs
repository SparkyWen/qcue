//! QCue — `DELETE /v1/account` (Apple Guideline 5.1.1(v): let users delete their account in-app).
//!
//! Revokes the caller's live sessions, then permanently purges their tenant. Every per-tenant table
//! FKs `tenants(id) ON DELETE CASCADE`, so a single scoped `DELETE FROM tenants WHERE id=$1` tears
//! down all associated data. The `tenants` root table has no RLS and is owned by the app role, so
//! the delete runs on the request's tenant-bound tx like any other handler — the `WHERE id=$1`
//! (bound to the JWT-verified `ctx.tenant_id`) is the sole, non-widenable scope.
use crate::auth::audit::audit;
use crate::auth::routes::revoke_user_sessions;
use crate::error::ApiError;
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use axum::extract::State;
use axum::routing::delete;
use axum::{Json, Router};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/account", delete(delete_account))
}

/// DELETE /v1/account — permanently delete the authenticated caller's account and all of its data.
async fn delete_account(
    State(st): State<AppState>,
    mut ctx: TenantCtx,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Revoke live sessions first. The tenant delete below cascade-removes the session rows too, but
    // doing it up front records the revocation and invalidates tokens even if the purge is retried.
    revoke_user_sessions(&st, ctx.tenant_id, ctx.user_id).await?;

    // Audit BEFORE the purge, and on its own auth_pool tx. `audit_log` FK-references `tenants(id)`,
    // so the row must be written while the tenant still exists; the DELETE below then cascade-removes
    // it (audit_log is tenant-scoped → the record is inherently ephemeral, retained only in tracing on
    // failure). Auditing AFTER the (uncommitted) DELETE would FK-DEADLOCK: the audit INSERT on a 2nd
    // connection would wait on the tenant row this tx is deleting, while this tx waits for audit to
    // return before it can commit. audit() is best-effort and never returns an error.
    audit(
        &st.auth_pool,
        Some(ctx.tenant_id),
        Some(ctx.user_id),
        "account.delete",
        None,
        serde_json::json!({}),
    )
    .await;

    // The single teardown statement: scoped to the JWT-verified tenant id, it cascades through every
    // per-tenant FK (ideas, wiki_*, messages, provider_credentials, sessions, …) by ON DELETE CASCADE.
    sqlx::query("DELETE FROM tenants WHERE id=$1")
        .bind(ctx.tenant_id)
        .execute(&mut *ctx.tx)
        .await?;
    ctx.tx.commit().await?;

    // Postgres only mirrors the structure; the user's actual content lives on disk under the tenant's
    // object-store dir — the wiki vault `<data_root>/objects/t/<tenant>/u/_/*.md` (the body SOURCE OF
    // TRUTH, S1) and the raw capture logs `.../u/<user>/captures/*.jsonl`. Purge that whole subtree so
    // "delete all data" is true on disk, not just in the index. `tenant_id` is a Uuid → a safe single
    // path segment (no traversal). Best-effort: the account is already gone from Postgres (the source
    // of truth for "exists"), so a failed unlink is LOGGED for ops cleanup, never silently dropped and
    // never fatal to the response (returning 500 here is useless — the caller's token is now revoked).
    let tenant_dir = std::path::PathBuf::from(&st.cfg.data_root)
        .join("objects")
        .join("t")
        .join(ctx.tenant_id.to_string());
    match tokio::fs::remove_dir_all(&tenant_dir).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {} // never had on-disk content
        Err(e) => tracing::warn!(
            target: "account",
            tenant = %ctx.tenant_id,
            error = %e,
            "account.delete: on-disk purge failed — orphaned files need manual cleanup"
        ),
    }

    // Durable success-path record (PII-free: the tenant id is an opaque uuid). The audit_log row above
    // is cascade-removed with the tenant, so this tracing line is the only surviving proof a deletion
    // occurred — needed for erasure/abuse forensics.
    tracing::info!(target: "account", tenant = %ctx.tenant_id, "account.delete: tenant purged");
    Ok(Json(serde_json::json!({ "ok": true })))
}
