// QCue S1-R39 — error taxonomy + classification result. No logic here (logic lives in router::classify).
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use ts_rs::TS;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum FailoverReason {
    Auth,
    AuthPermanent,
    Billing,
    RateLimit,
    Overloaded,
    ServerError,
    Timeout,
    ContextOverflow,
    PayloadTooLarge,
    ModelNotFound,
    ContentPolicyBlocked,
    FormatError,
    Unknown,
}

/// The classifier output: what-went-wrong (`reason`) decoupled from what-to-do (the 4 bits).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClassifiedError {
    pub reason: FailoverReason,
    pub status_code: Option<u16>,
    pub retryable: bool,
    pub should_compress: bool,
    pub should_rotate_credential: bool,
    pub should_fallback: bool,
    pub reset_at_ms: Option<i64>,
}

/// Raw transport/provider error surfaced to the classifier.
#[derive(Clone, Debug, Error, Serialize, Deserialize)]
pub enum ApiError {
    #[error("http status {status}: {body}")]
    Status { status: u16, #[serde(default)] body: String },
    #[error("stream idle timeout after {idle_ms}ms")]
    StreamIdle { idle_ms: u64 },
    #[error("stream ttfb timeout after {ttfb_ms}ms")]
    StreamTtfb { ttfb_ms: u64 },
    #[error("transport: {0}")]
    Transport(String),
    #[error("decode: {0}")]
    Decode(String),
}

/// Non-stream normalization failure.
#[derive(Clone, Debug, Error, Serialize, Deserialize)]
pub enum TransportError {
    #[error("missing field {0}")]
    MissingField(String),
    #[error("bad shape: {0}")]
    BadShape(String),
}

impl ApiError {
    /// The raw JSON error body, if any, for nested-body extraction (S1-R41).
    pub fn body_json(&self) -> Option<Value> {
        if let ApiError::Status { body, .. } = self {
            serde_json::from_str(body).ok()
        } else {
            None
        }
    }
}
