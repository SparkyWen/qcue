//! QCue S3-R17/R18/R19 — the BYOK keys vault management API. The plaintext key is NEVER persisted,
//! logged, or returned (B-R13): on write it is sealed (envelope) and only the ciphertext + `key_hint`
//! are stored; on read only `provider`, `label`, `key_hint`, `status`, `cooldown_until` are echoed.
//! Every response value is routed through the central `redact()` boundary before it leaves the server.
use crate::auth::audit::audit;
use crate::error::ApiError;
use crate::redact::redact_json;
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use axum::extract::{Path, State};
use axum::routing::{delete, put};
use axum::{Json, Router};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/settings/keys", put(put_key).get(list_keys))
        .route("/v1/settings/keys/{id}", delete(delete_key))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PutKey {
    provider: String,
    #[serde(default)]
    label: Option<String>,
    key: String,
    #[serde(default)]
    priority: i32,
}

/// Pass every outbound vault body through the central redactor (defense in depth: even if a future
/// field accidentally carried key-shaped text, it is scrubbed before it leaves the server).
fn redacted(mut v: serde_json::Value) -> Json<serde_json::Value> {
    redact_json(&mut v);
    Json(v)
}

/// PUT /settings/keys — seal the BYOK key (envelope) + store ciphertext + key_hint. Returns the hint
/// only; the plaintext never touches the DB in cleartext, a log line, or the response (S3-R17, B-R13).
async fn put_key(
    State(st): State<AppState>,
    mut ctx: TenantCtx,
    Json(req): Json<PutKey>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let sealed = st.secrets.seal(ctx.tenant_id, &req.key).await.map_err(ApiError::Other)?;
    // upsert by (tenant, provider, key_hint): re-PUTting the same key refreshes the row, not a dup.
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO provider_credentials \
             (tenant_id,provider,label,priority,key_ciphertext,key_nonce,key_tag,dek_wrapped,kek_id,key_hint,status) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'ok') \
         ON CONFLICT (tenant_id, provider, key_hint) DO UPDATE SET \
             label=EXCLUDED.label, priority=EXCLUDED.priority, key_ciphertext=EXCLUDED.key_ciphertext, \
             key_nonce=EXCLUDED.key_nonce, key_tag=EXCLUDED.key_tag, dek_wrapped=EXCLUDED.dek_wrapped, \
             kek_id=EXCLUDED.kek_id, status='ok', cooldown_until=NULL, dead_at=NULL \
         RETURNING id",
    )
    .bind(ctx.tenant_id)
    .bind(&req.provider)
    .bind(&req.label)
    .bind(req.priority)
    .bind(&sealed.key_ciphertext)
    .bind(&sealed.key_nonce)
    .bind(&sealed.key_tag)
    .bind(&sealed.dek_wrapped)
    .bind(&sealed.kek_id)
    .bind(&sealed.key_hint)
    .fetch_one(&mut *ctx.tx)
    .await?;
    // audit echoes only the hint (B-R11); plaintext never logged.
    audit(
        &st.auth_pool,
        Some(ctx.tenant_id),
        Some(ctx.user_id),
        "cred.add",
        None,
        serde_json::json!({"provider": req.provider, "key_hint": sealed.key_hint}),
    )
    .await;
    ctx.tx.commit().await?;
    Ok(redacted(serde_json::json!({
        "id": id, "provider": req.provider, "label": req.label,
        "key_hint": sealed.key_hint, "status": "ok"
    })))
}

/// GET /settings/keys — list the tenant's keys. NEVER returns the secret/ciphertext (S3-R18): only
/// provider, label, key_hint, status, cooldown_until.
async fn list_keys(mut ctx: TenantCtx) -> Result<Json<serde_json::Value>, ApiError> {
    // Self-heal on read (S1-R32..R35): a cooldown is hard-capped at MAX_COOLDOWN_MS (5 min) and the
    // pool treats an Exhausted cred as eligible again once `cooldown_until` passes — but the DB row
    // only flips back to `ok` when that key is *re-used and succeeds*. A key that is never re-exercised
    // stayed `exhausted` forever, so the settings badge showed it "cooling down" indefinitely (the
    // operator-reported bug). Clear any elapsed cooldown here so the DB, the pool, and the UI agree.
    // RLS-scoped via the tenant-bound tx; actively-cooling (future cooldown) and `dead` rows untouched.
    sqlx::query(
        "UPDATE provider_credentials \
            SET status='ok', cooldown_until=NULL \
          WHERE status='exhausted' AND cooldown_until IS NOT NULL AND cooldown_until <= now()",
    )
    .execute(&mut *ctx.tx)
    .await?;
    let rows = sqlx::query(
        "SELECT id,provider,label,key_hint,status::text AS status,cooldown_until \
         FROM provider_credentials ORDER BY provider, priority",
    )
    .fetch_all(&mut *ctx.tx)
    .await?;
    let items: Vec<_> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "provider": r.get::<String, _>("provider"),
                "label": r.get::<Option<String>, _>("label"),
                "key_hint": r.get::<String, _>("key_hint"),
                "status": r.get::<String, _>("status"),
                "cooldown_until": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("cooldown_until"),
            })
        })
        .collect();
    ctx.tx.commit().await?;
    Ok(redacted(serde_json::json!({ "keys": items })))
}

/// DELETE /settings/keys/{id} — remove a key. RLS-scoped: a tenant can only delete its own rows.
async fn delete_key(
    State(st): State<AppState>,
    mut ctx: TenantCtx,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    sqlx::query("DELETE FROM provider_credentials WHERE id=$1")
        .bind(id)
        .execute(&mut *ctx.tx)
        .await?;
    audit(
        &st.auth_pool,
        Some(ctx.tenant_id),
        Some(ctx.user_id),
        "cred.delete",
        None,
        serde_json::json!({ "id": id }),
    )
    .await;
    ctx.tx.commit().await?;
    Ok(redacted(serde_json::json!({ "deleted": id })))
}
