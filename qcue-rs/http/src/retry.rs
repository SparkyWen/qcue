// QCue S1-R45 — backoff(base, attempt) = base * 2^(attempt-1) * jitter(0.9..1.1), capped, Retry-After wins.
// Jitter entropy comes from Uuid::new_v4() bytes (avoids pulling in `rand`).
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub base_ms: u64,
    pub max_ms: u64,
}
impl Default for RetryPolicy {
    fn default() -> Self {
        Self { base_ms: 500, max_ms: 60_000 }
    }
}

fn jitter_factor_milli() -> u64 {
    // Returns a value in [900, 1100] (i.e. 0.9..1.1 scaled by 1000) from a fresh UUID byte.
    let byte = Uuid::new_v4().as_bytes()[0] as u64; // 0..=255
    900 + (byte * 200 / 255) // 900..=1100
}

/// `reset_at_ms` (a provider Retry-After / reset hint, in ms) overrides the computed value.
pub fn backoff_delay_ms(p: &RetryPolicy, attempt: u32, reset_at_ms: Option<i64>) -> u64 {
    // Plan note: clippy::collapsible_if requires the nested `if let`/`if` be a let-chain
    // (edition-2024). Behavior is identical to the plan's nested form.
    if let Some(r) = reset_at_ms
        && r >= 0
    {
        return (r as u64).min(p.max_ms.max(r as u64));
    }
    let exp = p
        .base_ms
        .saturating_mul(1u64 << (attempt.saturating_sub(1)).min(20));
    let jittered = exp.saturating_mul(jitter_factor_milli()) / 1000;
    jittered.min(p.max_ms)
}
