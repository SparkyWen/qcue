// QCue S1-R45 — exponential backoff with UUID-derived jitter; Retry-After/reset_at wins.
use http::retry::{backoff_delay_ms, RetryPolicy};

#[test]
fn test_backoff_grows_within_jitter_band() {
    let p = RetryPolicy { base_ms: 500, max_ms: 60_000 };
    let d1 = backoff_delay_ms(&p, 1, None);
    let d2 = backoff_delay_ms(&p, 2, None);
    let d3 = backoff_delay_ms(&p, 3, None);
    // attempt 1 ≈ 500 * [0.9,1.1], attempt 2 ≈ 1000, attempt 3 ≈ 2000.
    assert!((450..=550).contains(&d1), "d1={d1}");
    assert!((900..=1100).contains(&d2), "d2={d2}");
    assert!((1800..=2200).contains(&d3), "d3={d3}");
}

#[test]
fn test_backoff_capped() {
    let p = RetryPolicy { base_ms: 1000, max_ms: 5000 };
    let d = backoff_delay_ms(&p, 10, None); // 1000 * 2^9 would be huge; capped at 5000.
    assert!(d <= 5000);
}

#[test]
fn test_retry_after_overrides_computed() {
    let p = RetryPolicy { base_ms: 500, max_ms: 60_000 };
    // a 30s reset hint wins over the computed ~500ms.
    let d = backoff_delay_ms(&p, 1, Some(30_000));
    assert_eq!(d, 30_000);
}
