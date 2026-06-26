#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R39..R42 — classify maps error bodies to reasons + 4 action bits.
use protocol::{ApiError, FailoverReason};
use router::classify::{ClassifyCtx, classify};

fn ctx() -> ClassifyCtx {
    ClassifyCtx::default()
}

#[test]
fn test_mvp_reasons_table() {
    let cases = [
        (
            ApiError::Status {
                status: 401,
                body: "{\"error\":{\"message\":\"invalid api key\"}}".into(),
            },
            FailoverReason::Auth,
        ),
        (
            ApiError::Status {
                status: 403,
                body: "{\"error\":{\"message\":\"token_revoked\"}}".into(),
            },
            FailoverReason::AuthPermanent,
        ),
        (
            ApiError::Status { status: 500, body: "server error".into() },
            FailoverReason::ServerError,
        ),
        (
            ApiError::Status { status: 529, body: "overloaded".into() },
            FailoverReason::Overloaded,
        ),
        (ApiError::StreamIdle { idle_ms: 30000 }, FailoverReason::Timeout),
        (
            ApiError::Status { status: 400, body: "context length exceeded".into() },
            FailoverReason::ContextOverflow,
        ),
        (
            ApiError::Status { status: 400, body: "content policy violation".into() },
            FailoverReason::ContentPolicyBlocked,
        ),
    ];
    for (err, want) in cases {
        assert_eq!(classify(&err, &ctx()).reason, want, "for {err:?}");
    }
}

#[test]
fn test_billing_vs_ratelimit() {
    // S1-R40 — bare "quota exceeded" → Billing (not retryable); + transient signal → RateLimit (rotate).
    let billing = classify(
        &ApiError::Status {
            status: 429,
            body: "{\"error\":{\"message\":\"You exceeded your current quota\"}}".into(),
        },
        &ctx(),
    );
    assert_eq!(billing.reason, FailoverReason::Billing);
    assert!(!billing.retryable);

    let rl = classify(
        &ApiError::Status {
            status: 429,
            body: "{\"error\":{\"message\":\"quota exceeded, try again in 30s\"}}".into(),
        },
        &ctx(),
    );
    assert_eq!(rl.reason, FailoverReason::RateLimit);
    assert!(rl.should_rotate_credential);
}

#[test]
fn test_nested_body_extraction() {
    // S1-R41 — OpenRouter wraps the upstream error in metadata.raw.
    let wrapped = ApiError::Status {
        status: 200,
        body: "{\"error\":{\"metadata\":{\"raw\":\"{\\\"error\\\":{\\\"message\\\":\\\"rate limit, retry after 5s\\\"}}\"}}}".into(),
    };
    let ce = classify(&wrapped, &ctx());
    assert_eq!(ce.reason, FailoverReason::RateLimit);
}

#[test]
fn test_nonretryable_aborts() {
    // S1-R42 — ContentPolicyBlocked + AuthPermanent are retryable=false.
    let cp = classify(
        &ApiError::Status { status: 400, body: "content policy violation".into() },
        &ctx(),
    );
    assert!(!cp.retryable && !cp.should_rotate_credential && !cp.should_fallback);
    let ap = classify(
        &ApiError::Status { status: 403, body: "invalid_grant".into() },
        &ctx(),
    );
    assert!(!ap.retryable);
}

#[test]
fn test_ratelimit_extracts_reset_delay() {
    let rl = classify(
        &ApiError::Status { status: 429, body: "rate limited, retry after 12s".into() },
        &ctx(),
    );
    assert_eq!(rl.reset_at_ms, Some(12_000));
}

#[test]
fn test_retry_after_ms_from_ctx_wins() {
    // S1-R34 — the HTTP Retry-After header (threaded as ctx.retry_after_ms) sets reset_at_ms.
    let ce = classify(
        &ApiError::Status { status: 429, body: "{\"error\":{\"message\":\"slow down, try again in 30s\"}}".into() },
        &ClassifyCtx { provider: "openai".into(), retry_after_ms: Some(120_000) },
    );
    assert_eq!(ce.reset_at_ms, Some(120_000), "header value overrides the body hint");
}

#[test]
fn test_extract_reset_ms_parses_hours() {
    // S1-R34 — "retry after 2h" is understood (was silently ignored before).
    let ce = classify(
        &ApiError::Status { status: 429, body: "{\"error\":{\"message\":\"quota exceeded, retry after 2h\"}}".into() },
        &ClassifyCtx { provider: "openai".into(), retry_after_ms: None },
    );
    assert_eq!(ce.reset_at_ms, Some(2 * 3_600_000));
}
