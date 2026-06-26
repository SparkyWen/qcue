//! QCue REC-R3/REC-R4 — recall conversation history reads, tenant-scoped (FORCE RLS) + redacted:
//!   - `GET /v1/conversations`                    → `{conversations:[ConversationSummary]}` newest first
//!   - `GET /v1/conversations/{thread}/messages`  → `{messages:[ConversationMessage]}` in seq order
//!
//! Both are TenantCtx routes (the JWT binds `app.tenant_id`); the body is passed through `redact_json`
//! (S1-R38) before it leaves the server. Persisted `messages` already exclude tool_calls/provider_data
//! (REC-D6), so the read carries only `role`+`content`.
use crate::error::ApiError;
use crate::redact::redact_json;
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use store::messages_repo::{ConversationsRepo, MessagesRepo};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/conversations", get(list_conversations))
        .route("/v1/conversations/{thread}/messages", get(get_messages))
}

fn redacted(mut v: serde_json::Value) -> Json<serde_json::Value> {
    redact_json(&mut v);
    Json(v)
}

/// `GET /v1/conversations` — the tenant's recall threads, newest first (REC-R3). The `TenantCtx`
/// extractor already bound the GUC; we commit its tx then read through the repo's own GUC-bound tx
/// (like the recall driver) for the list+snippet.
async fn list_conversations(
    State(st): State<AppState>,
    ctx: TenantCtx,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tenant = ctx.tenant_id;
    ctx.tx.commit().await?;
    let rows = ConversationsRepo::new(st.pool.clone()).list(tenant).await?;
    let conversations: Vec<_> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "title": r.title,
                "updated_at": r.updated_at.to_rfc3339(),
                "last_snippet": r.last_snippet,
            })
        })
        .collect();
    Ok(redacted(serde_json::json!({ "conversations": conversations })))
}

/// `GET /v1/conversations/{thread}/messages` — the thread's persisted turns in seq order (REC-R4).
/// Only `role`+`content` are returned (no tool_calls/provider_data — they were never persisted, REC-D6).
async fn get_messages(
    State(st): State<AppState>,
    ctx: TenantCtx,
    Path(thread): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tenant = ctx.tenant_id;
    ctx.tx.commit().await?;
    let rows = MessagesRepo::new(st.pool.clone()).read_session(tenant, thread).await?;
    let messages: Vec<_> = rows
        .into_iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content.unwrap_or_default() }))
        .collect();
    Ok(redacted(serde_json::json!({ "messages": messages })))
}
