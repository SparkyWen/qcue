//! QCue S3 — `GET /v1/jobs/{id}` poll: the durable TaskRecord state+result, RLS-scoped to the caller.
use crate::error::ApiError;
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use axum::extract::Path;
use axum::routing::get;
use axum::{Json, Router};
use sqlx::Row;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/jobs/{id}", get(get_job))
}

async fn get_job(mut ctx: TenantCtx, Path(id): Path<Uuid>) -> Result<Json<serde_json::Value>, ApiError> {
    let row = sqlx::query(
        "SELECT id, kind::text AS kind, state::text AS state, attempt_count, max_attempts, \
                result, last_error, updated_at FROM jobs WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    let out = match row {
        Some(r) => serde_json::json!({
            "id": r.get::<Uuid, _>("id"),
            "kind": r.get::<String, _>("kind"),
            "state": r.get::<String, _>("state"),
            "attempt_count": r.get::<i32, _>("attempt_count"),
            "max_attempts": r.get::<i32, _>("max_attempts"),
            "result": r.get::<Option<serde_json::Value>, _>("result"),
            "last_error": r.get::<Option<String>, _>("last_error"),
            "updated_at": r.get::<chrono::DateTime<chrono::Utc>, _>("updated_at"),
        }),
        None => {
            ctx.tx.commit().await?;
            return Err(ApiError::NotFound);
        }
    };
    ctx.tx.commit().await?;
    Ok(Json(out))
}
