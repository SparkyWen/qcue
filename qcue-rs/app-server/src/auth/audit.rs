//! QCue S3-R16 — audit writer that NEVER blocks the auth decision (swallow + warn on failure). B-R11 redaction applied.
//!
//! The `audit_log` table (Appendix B §4.20) lands with the M2 set, so this writer now performs a real
//! INSERT. The write is INFALLIBLE with respect to the caller: any error (table absent, RLS reject,
//! pool exhausted, forced test failure) is swallowed + logged, never propagated — so a failed audit
//! can never change or block the auth outcome (S3-R16). The `detail` JSON passes through the central
//! redactor first (B-R11) so no secret can ever reach `audit_log.detail`.
//!
//! `audit_log` has FORCE ROW LEVEL SECURITY, so the INSERT runs inside its own short-lived tx with
//! `app.tenant_id` bound to the row's `tenant_id` (the RLS `WITH CHECK` predicate). A `None` tenant
//! (e.g. an `auth.login.failed` for an unknown email) cannot satisfy `tenant_id = app_tenant()`, so
//! that INSERT is rejected by RLS — and harmlessly swallowed, exactly as the non-blocking invariant
//! requires.
use crate::redact::redact_json;
use sqlx::PgPool;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

/// Test seam (S3-R16): force every audit INSERT to fail, so the "audit never blocks auth" invariant
/// can be exercised even when the table exists. Always `false` in production (nothing flips it).
static AUDIT_FAIL: AtomicBool = AtomicBool::new(false);

/// Force-fail the audit writer (tests only). Returns the previous value.
pub fn set_audit_fail(v: bool) -> bool {
    AUDIT_FAIL.swap(v, Ordering::SeqCst)
}

/// Write one audit row. Non-blocking: any failure is swallowed + warned, never returned to the caller.
pub async fn audit(
    pool: &PgPool,
    tenant_id: Option<Uuid>,
    user_id: Option<Uuid>,
    action: &str,
    ip_hash: Option<&str>,
    mut detail: serde_json::Value,
) {
    redact_json(&mut detail); // B-R11: never let a key reach audit_log.detail
    if let Err(e) = write_audit_row(pool, tenant_id, user_id, action, ip_hash, &detail).await {
        tracing::warn!(target: "audit", error=%e, action, "audit.write_failed");
    }
}

/// The fallible inner write (its `Result` is consumed by `audit`, never escaping to auth).
async fn write_audit_row(
    pool: &PgPool,
    tenant_id: Option<Uuid>,
    user_id: Option<Uuid>,
    action: &str,
    ip_hash: Option<&str>,
    detail: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    if AUDIT_FAIL.load(Ordering::SeqCst) {
        return Err(sqlx::Error::Configuration("audit forced-fail (test seam)".into()));
    }
    let mut tx = pool.begin().await?;
    // audit_log is FORCE RLS: bind app.tenant_id so the row's tenant_id satisfies WITH CHECK. A None
    // tenant binds the empty GUC → app_tenant() is NULL → the INSERT is RLS-rejected (fail closed).
    let guc = tenant_id.map(|t| t.to_string()).unwrap_or_default();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(&guc)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO audit_log(tenant_id, user_id, action, detail, ip_hash) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(action)
    .bind(detail)
    .bind(ip_hash)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
