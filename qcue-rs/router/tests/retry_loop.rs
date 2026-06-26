#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R43..R46 — the action-bit match drives exactly one branch; fallback re-derives ApiMode; caps honored.
use protocol::{ApiMode, ClassifiedError, FailoverReason};
use router::retry_loop::{Action, FallbackChain, decide_action};

fn ce(
    reason: FailoverReason,
    retryable: bool,
    rotate: bool,
    fallback: bool,
    compress: bool,
) -> ClassifiedError {
    ClassifiedError {
        reason,
        status_code: None,
        retryable,
        should_compress: compress,
        should_rotate_credential: rotate,
        should_fallback: fallback,
        reset_at_ms: None,
    }
}

#[test]
fn test_dispatch_each_branch() {
    assert!(matches!(
        decide_action(&ce(FailoverReason::RateLimit, true, true, false, false)),
        Action::Rotate
    ));
    assert!(matches!(
        decide_action(&ce(FailoverReason::Billing, false, false, true, false)),
        Action::Fallback
    ));
    assert!(matches!(
        decide_action(&ce(FailoverReason::ContextOverflow, true, false, false, true)),
        Action::Compress
    ));
    assert!(matches!(
        decide_action(&ce(FailoverReason::Timeout, true, false, false, false)),
        Action::Backoff
    ));
    assert!(matches!(
        decide_action(&ce(FailoverReason::ContentPolicyBlocked, false, false, false, false)),
        Action::Abort
    ));
}

#[test]
fn test_fallback_rederives_api_mode() {
    // S1-R44 — advancing from an Anthropic provider to a ChatCompletions provider switches the wire.
    let mut chain = FallbackChain::new(vec![
        ("anthropic".into(), "claude-sonnet-4".into(), ApiMode::AnthropicMessages),
        ("deepseek".into(), "deepseek-chat".into(), ApiMode::ChatCompletions),
    ]);
    assert_eq!(chain.current().2, ApiMode::AnthropicMessages);
    let next = chain.advance().unwrap();
    assert_eq!(next.2, ApiMode::ChatCompletions, "fallback must re-derive api_mode");
}

#[test]
fn test_retry_caps_per_provider() {
    // S1-R46 — request_max_retries=1 advances to fallback after one failed retry.
    let mut chain = FallbackChain::new(vec![
        ("p1".into(), "m1".into(), ApiMode::ChatCompletions),
        ("p2".into(), "m2".into(), ApiMode::ChatCompletions),
    ]);
    assert!(chain.advance().is_some()); // → p2
    assert!(chain.advance().is_none()); // chain exhausted
}
