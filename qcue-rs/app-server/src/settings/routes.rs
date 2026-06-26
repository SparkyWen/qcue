//! QCue S3 — the Settings READ/WRITE surface the Flutter client consumes beyond the BYOK vault (the
//! contract in `qcue_app/lib/core/net/http_api_client.dart`):
//!   - `GET /v1/settings/models/{provider}`          → `{models:[String]}` selectable models
//!   - `GET /v1/settings/models/{provider}/active`   → `{model:String}` active pick (404 ⇒ null)
//!   - `PUT /v1/settings/models/{provider}/active`   `{model}` → persist the active pick
//!   - `GET /v1/settings/dream`                      → `{enabled:bool}` the D9 server-Dream posture
//!   - `PUT /v1/settings/dream`                      `{enabled}` → toggle the D9 posture
//!
//! Models: there is no live `fetch_models` wired yet (the provider profiles carry empty fallback lists),
//! so the selectable set is a small STATIC per-provider catalog (D7). The active pick persists per
//! (tenant, provider) in `session_kv` (key `model:<provider>`) under a stable SETTINGS session — reusing
//! the existing versioned-KV table rather than a new table, all under FORCE RLS via the per-request GUC.
//! The server-Dream posture (D9) persists in the SAME `session_kv` settings session (key
//! `settings:server_dream`) — `tenants.dream_enabled` is SELECT-only for the app role (no UPDATE grant),
//! so the versioned KV is the writable, RLS-clean home for the toggle (read==write, no grant pitfall).
use crate::error::ApiError;
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use axum::extract::Path;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/settings/models/{provider}", get(list_models))
        .route(
            "/v1/settings/models/{provider}/active",
            get(get_active).put(set_active),
        )
        .route("/v1/settings/dream", get(get_dream).put(set_dream))
}

/// The settings blackboard session: a fixed nil-UUID `session_id` so per-(tenant,provider) settings
/// reuse `session_kv`'s `UNIQUE (tenant_id, session_id, key)` without a per-session row (D-settings).
const SETTINGS_SESSION: Uuid = Uuid::nil();

/// A small static per-provider model catalog (D7). No live `fetch_models` is wired (the provider
/// profiles carry empty fallback lists), so this is the selectable set the picker offers. An unknown
/// provider yields an empty list (the client renders "no models"). The catalog is owned by
/// `crate::dispatch` so the model picker and the per-tenant route resolver share ONE source of truth.
fn static_catalog(provider: &str) -> Vec<&'static str> {
    crate::dispatch::provider_models(provider)
}

/// `GET /v1/settings/models/{provider}` — the selectable models for a provider (static catalog).
async fn list_models(
    ctx: TenantCtx,
    Path(provider): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // No DB read needed, but commit the extractor's open tx so the connection is released cleanly.
    ctx.tx.commit().await?;
    let models = static_catalog(&provider);
    Ok(Json(serde_json::json!({ "models": models })))
}

/// `GET /v1/settings/models/{provider}/active` — the active model pick for a provider, or 404 (⇒ null
/// on the client) if none chosen. Stored in `session_kv` under the settings session, key `model:<p>`.
async fn get_active(
    mut ctx: TenantCtx,
    Path(provider): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let key = format!("model:{provider}");
    let val: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT value FROM session_kv WHERE session_id=$1 AND key=$2",
    )
    .bind(SETTINGS_SESSION)
    .bind(&key)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    ctx.tx.commit().await?;
    let model = val.and_then(|v| v.get("model").and_then(|m| m.as_str()).map(str::to_string));
    match model {
        Some(m) => Ok(Json(serde_json::json!({ "model": m }))),
        None => Err(ApiError::NotFound),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetActiveReq {
    model: String,
}

/// `PUT /v1/settings/models/{provider}/active` `{model}` — persist the active model for a provider.
/// Validated as ROUTABLE (RESP-R10): in the curated catalog OR a known family variant, so a user can pick
/// a newer gpt-5.x/claude variant without a 400; only a truly foreign id (or unknown provider) is rejected.
async fn set_active(
    mut ctx: TenantCtx,
    Path(provider): Path<String>,
    Json(req): Json<SetActiveReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !crate::dispatch::is_routable_model(&provider, &req.model) {
        ctx.tx.commit().await?;
        return Err(ApiError::BadRequest(format!(
            "unknown model '{}' for provider '{provider}'",
            req.model
        )));
    }
    let key = format!("model:{provider}");
    let value = serde_json::json!({ "model": req.model });
    sqlx::query(
        "INSERT INTO session_kv (tenant_id, session_id, key, value) VALUES ($1,$2,$3,$4) \
         ON CONFLICT (tenant_id, session_id, key) DO UPDATE \
           SET value=EXCLUDED.value, version=session_kv.version+1, updated_at=now()",
    )
    .bind(ctx.tenant_id)
    .bind(SETTINGS_SESSION)
    .bind(&key)
    .bind(&value)
    .execute(&mut *ctx.tx)
    .await?;
    ctx.tx.commit().await?;
    Ok(Json(serde_json::json!({ "model": req.model })))
}

/// The `session_kv` key for the per-tenant D9 server-Dream posture (settings session).
const DREAM_KEY: &str = "settings:server_dream";

/// `GET /v1/settings/dream` — the D9 server-readable / server-Dream posture (per-tenant). Reads the
/// `session_kv` settings toggle; an absent toggle defaults to OFF (fail-closed for the server-readable
/// posture — the server only reads/dreams over the wiki once the user has explicitly opted in, D9).
async fn get_dream(mut ctx: TenantCtx) -> Result<Json<serde_json::Value>, ApiError> {
    let val: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT value FROM session_kv WHERE session_id=$1 AND key=$2",
    )
    .bind(SETTINGS_SESSION)
    .bind(DREAM_KEY)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    ctx.tx.commit().await?;
    let enabled = val
        .and_then(|v| v.get("enabled").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    Ok(Json(serde_json::json!({ "enabled": enabled })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetDreamReq {
    enabled: bool,
}

/// `PUT /v1/settings/dream` `{enabled}` — toggle the D9 server-Dream posture for the caller's tenant.
/// Persisted in the `session_kv` settings session (versioned upsert), tenant-scoped under FORCE RLS.
async fn set_dream(
    mut ctx: TenantCtx,
    Json(req): Json<SetDreamReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let value = serde_json::json!({ "enabled": req.enabled });
    sqlx::query(
        "INSERT INTO session_kv (tenant_id, session_id, key, value) VALUES ($1,$2,$3,$4) \
         ON CONFLICT (tenant_id, session_id, key) DO UPDATE \
           SET value=EXCLUDED.value, version=session_kv.version+1, updated_at=now()",
    )
    .bind(ctx.tenant_id)
    .bind(SETTINGS_SESSION)
    .bind(DREAM_KEY)
    .bind(&value)
    .execute(&mut *ctx.tx)
    .await?;
    ctx.tx.commit().await?;
    Ok(Json(serde_json::json!({ "enabled": req.enabled })))
}
