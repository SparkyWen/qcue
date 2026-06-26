// QCue S1-R39..R42 — classify: what-went-wrong → 4 action bits. The retry loop never re-classifies.
use protocol::{ApiError, ClassifiedError, FailoverReason};

#[derive(Clone, Debug, Default)]
pub struct ClassifyCtx {
    pub provider: String,
    /// S1-R34 — the parsed HTTP `Retry-After` (ms-from-now) when the response carried one. Takes
    /// precedence over the body-message heuristic.
    pub retry_after_ms: Option<i64>,
}

const TERMINAL_AUTH: &[&str] = &[
    "token_revoked",
    "invalid_grant",
    "token_invalidated",
    "account_deactivated",
];
const TRANSIENT_SIGNALS: &[&str] = &[
    "try again",
    "retry after",
    "resets at",
    "resets in",
    "requests remaining",
    "rate limit",
    "too many requests",
];
const BILLING_SIGNALS: &[&str] = &[
    "quota",
    "insufficient_quota",
    "billing",
    "exceeded your current quota",
    "payment",
    "credit",
];

/// Pull the deepest error message out of nested vendor bodies (S1-R41).
fn extract_message(err: &ApiError) -> String {
    let mut msg = match err {
        ApiError::Status { body, .. } => body.clone(),
        ApiError::StreamIdle { .. } | ApiError::StreamTtfb { .. } => "stream timeout".into(),
        ApiError::Transport(s) | ApiError::Decode(s) => s.clone(),
    };
    // unwrap OpenRouter metadata.raw and standard error.message up to a few levels.
    if let Some(v) = err.body_json() {
        if let Some(raw) = v.pointer("/error/metadata/raw").and_then(|x| x.as_str()) {
            msg = raw.to_string();
            if let Ok(inner) = serde_json::from_str::<serde_json::Value>(raw)
                && let Some(m) = inner.pointer("/error/message").and_then(|x| x.as_str())
            {
                msg = m.to_string();
            }
        } else if let Some(m) = v.pointer("/error/message").and_then(|x| x.as_str()) {
            msg = m.to_string();
        }
    }
    msg.to_ascii_lowercase()
}

/// Parse a reset delay (seconds) from common phrasings → ms (S1-R32/R40).
fn extract_reset_ms(msg: &str) -> Option<i64> {
    // "retry after 12s" / "try again in 30s" / "resets in 5 seconds"
    let bytes = msg.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let num: i64 = msg[start..i].parse().unwrap_or(0);
            let rest = msg[i..].trim_start();
            // saturating: a huge digit run in a hostile error string must not overflow the multiply.
            if rest.starts_with('s') || rest.starts_with("sec") {
                return Some(num.saturating_mul(1000));
            }
            if rest.starts_with('m') && !rest.starts_with("ms") {
                return Some(num.saturating_mul(60_000));
            }
            if rest.starts_with('h') {
                return Some(num.saturating_mul(3_600_000));
            }
        } else {
            i += 1;
        }
    }
    None
}

pub fn classify(err: &ApiError, ctx: &ClassifyCtx) -> ClassifiedError {
    let status = match err {
        ApiError::Status { status, .. } => Some(*status),
        _ => None,
    };
    let msg = extract_message(err);
    let reset_at_ms = ctx.retry_after_ms.or_else(|| extract_reset_ms(&msg));
    let reason = classify_reason(err, status, &msg);
    let (retryable, rotate, fallback, compress) = action_bits(reason);
    ClassifiedError {
        reason,
        status_code: status,
        retryable,
        should_rotate_credential: rotate,
        should_fallback: fallback,
        should_compress: compress,
        reset_at_ms,
    }
}

fn classify_reason(err: &ApiError, status: Option<u16>, msg: &str) -> FailoverReason {
    if matches!(err, ApiError::StreamIdle { .. } | ApiError::StreamTtfb { .. }) {
        return FailoverReason::Timeout;
    }
    if TERMINAL_AUTH.iter().any(|s| msg.contains(s)) {
        return FailoverReason::AuthPermanent;
    }
    if msg.contains("content policy") || msg.contains("content_policy") {
        return FailoverReason::ContentPolicyBlocked;
    }
    if msg.contains("context length")
        || msg.contains("context_length")
        || msg.contains("maximum context")
    {
        return FailoverReason::ContextOverflow;
    }
    // billing vs rate-limit disambiguation (S1-R40): transient signal flips Billing → RateLimit.
    let billing_like = BILLING_SIGNALS.iter().any(|s| msg.contains(s));
    let transient = TRANSIENT_SIGNALS.iter().any(|s| msg.contains(s));
    match status {
        Some(401) => FailoverReason::Auth,
        Some(403) => {
            if TERMINAL_AUTH.iter().any(|s| msg.contains(s)) {
                FailoverReason::AuthPermanent
            } else {
                FailoverReason::Auth
            }
        }
        Some(404) => FailoverReason::ModelNotFound,
        Some(413) => FailoverReason::PayloadTooLarge,
        Some(429) => {
            if transient {
                FailoverReason::RateLimit
            } else if billing_like {
                FailoverReason::Billing
            } else {
                FailoverReason::RateLimit
            }
        }
        Some(529) => FailoverReason::Overloaded,
        Some(s) if s >= 500 => FailoverReason::ServerError,
        _ => {
            if transient {
                FailoverReason::RateLimit
            } else if billing_like {
                FailoverReason::Billing
            } else {
                FailoverReason::Unknown
            }
        }
    }
}

/// (retryable, should_rotate_credential, should_fallback, should_compress)
fn action_bits(reason: FailoverReason) -> (bool, bool, bool, bool) {
    match reason {
        FailoverReason::Auth => (true, true, false, false), // rotate to another key
        FailoverReason::AuthPermanent => (false, false, false, false),
        FailoverReason::Billing => (false, false, true, false), // fall back to another provider
        FailoverReason::RateLimit => (true, true, false, false),
        FailoverReason::Overloaded => (true, false, true, false),
        FailoverReason::ServerError => (true, false, true, false),
        FailoverReason::Timeout => (true, false, false, false),
        FailoverReason::ContextOverflow => (true, false, false, true), // compress same provider
        FailoverReason::PayloadTooLarge => (true, false, false, true),
        FailoverReason::ModelNotFound => (false, false, true, false),
        FailoverReason::ContentPolicyBlocked => (false, false, false, false),
        FailoverReason::FormatError => (true, false, false, false),
        FailoverReason::Unknown => (true, false, true, false),
    }
}
