// QCue S3 — ApiError → (HTTP status, JSON-RPC code). -32001 = overload/cost-cap (codex-rust.md §2).
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unprocessable: {0}")]
    Unprocessable(String),
    #[error("payload too large")]
    TooLarge,
    #[error("server overloaded; retry later")]
    Overloaded, // -32001
    #[error("daily cost cap reached")]
    CostCap, // -32001 family
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl ApiError {
    pub fn rpc_code(&self) -> i32 {
        match self {
            ApiError::Overloaded | ApiError::CostCap => -32001,
            ApiError::Unauthorized => -32002,
            ApiError::BadRequest(_) | ApiError::Unprocessable(_) => -32602,
            _ => -32603,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self {
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden => StatusCode::FORBIDDEN,
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unprocessable(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ApiError::Overloaded | ApiError::CostCap => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let code = self.rpc_code();
        // Never leak internal detail (raw SQL / anyhow chains) to the client (S3-R61). The DB/Other
        // variants carry an exact internal error — log it server-side at the boundary, but return a
        // stable generic message. The user-facing variants (BadRequest/Unprocessable/…) are already
        // safe, intentional messages and are surfaced verbatim.
        let message = match &self {
            ApiError::Db(e) => {
                tracing::error!(error = %e, "internal DB error");
                "internal server error".to_string()
            }
            ApiError::Other(e) => {
                tracing::error!(error = %e, "internal error");
                "internal server error".to_string()
            }
            other => other.to_string(),
        };
        (status, Json(serde_json::json!({"error": {"code": code, "message": message}}))).into_response()
    }
}
