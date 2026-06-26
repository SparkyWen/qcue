#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R30..R37 — 3-state pool: select skips Dead/cooled; DEAD≠EXHAUSTED; key_hint marks failer; TTL; strategies.
use protocol::CredStatus;
use router::pool::{CredentialPool, PoolStrategy, PooledCredential, ttl_for_status};

fn cred(hint: &str, status: CredStatus, priority: i32) -> PooledCredential {
    PooledCredential {
        id: uuid::Uuid::now_v7(),
        label: None,
        priority,
        status,
        key_hint: hint.into(),
        last_error_code: None,
        last_error_reason: None,
        request_count: 0,
    }
}

#[test]
fn test_select_skips_dead_and_cooled() {
    let now = 1_000_000i64;
    let mut pool = CredentialPool::new(
        vec![
            cred("dead", CredStatus::Dead, 0),
            cred("cool", CredStatus::Exhausted { until_ms: now + 60_000 }, 1),
            cred("ok", CredStatus::Ok, 2),
        ],
        PoolStrategy::FillFirst,
    );
    let sel = pool.select(now).unwrap();
    assert_eq!(sel.key_hint, "ok");
}

#[test]
fn test_cooled_becomes_eligible_after_until() {
    let now = 1_000_000i64;
    let mut pool = CredentialPool::new(
        vec![cred("a", CredStatus::Exhausted { until_ms: now - 1 }, 0)],
        PoolStrategy::FillFirst,
    );
    // past `until` → eligible again.
    assert!(pool.select(now).is_some());
}

#[test]
fn test_dead_excluded_forever() {
    // S1-R31 — a Dead credential is never selected across many cooldown cycles.
    let mut pool =
        CredentialPool::new(vec![cred("d", CredStatus::Dead, 0)], PoolStrategy::FillFirst);
    for cycle in 0..10 {
        assert!(pool.select(cycle * 3_600_000).is_none());
    }
}

#[test]
fn test_key_hint_marks_the_failer() {
    // S1-R33 — a 429 carrying key B's hint exhausts B; A stays Ok.
    let now = 1_000_000i64;
    let mut pool = CredentialPool::new(
        vec![cred("A", CredStatus::Ok, 0), cred("B", CredStatus::Ok, 1)],
        PoolStrategy::RoundRobin,
    );
    pool.mark_exhausted_and_rotate(429, Some(now + 3_600_000), Some("B"), now);
    let a = pool.find("A").unwrap();
    let b = pool.find("B").unwrap();
    assert_eq!(a.status, CredStatus::Ok);
    assert!(matches!(b.status, CredStatus::Exhausted { .. }));
}

#[test]
fn test_ttl_policy() {
    // S1-R32 — 401→30s, 429→60s, default→60s (self-heal posture).
    assert_eq!(ttl_for_status(401), 30 * 1000);
    assert_eq!(ttl_for_status(429), 60 * 1000);
    assert_eq!(ttl_for_status(500), 60 * 1000);
}

#[test]
fn test_mark_dead_terminal_auth() {
    let mut pool =
        CredentialPool::new(vec![cred("x", CredStatus::Ok, 0)], PoolStrategy::FillFirst);
    pool.mark_dead("x");
    assert_eq!(pool.find("x").unwrap().status, CredStatus::Dead);
}

#[test]
fn test_pool_strategies() {
    // S1-R36 — least_used picks lowest request_count; round_robin advances.
    let now = 0i64;
    let mut lu = CredentialPool::new(
        vec![
            PooledCredential { request_count: 5, ..cred("hi", CredStatus::Ok, 0) },
            PooledCredential { request_count: 1, ..cred("lo", CredStatus::Ok, 1) },
        ],
        PoolStrategy::LeastUsed,
    );
    assert_eq!(lu.select(now).unwrap().key_hint, "lo");
}

#[test]
fn test_soft_lease_caps_concurrency() {
    // S1-R35 — a maxed key (lease held) is skipped; the other is selected.
    let now = 0i64;
    let mut pool = CredentialPool::new(
        vec![cred("k1", CredStatus::Ok, 0), cred("k2", CredStatus::Ok, 1)],
        PoolStrategy::FillFirst,
    );
    let id1 = pool.find("k1").unwrap().id;
    pool.acquire_lease(Some(id1)); // k1 now at its per-key cap (default 1)
    let sel = pool.select(now).unwrap();
    assert_eq!(sel.key_hint, "k2");
}

#[test]
fn test_dead_pruned_after_24h() {
    // S1-R37 — a Dead credential is pruned from the active set after dead_at + 24h.
    let mut pool =
        CredentialPool::new(vec![cred("d", CredStatus::Dead, 0)], PoolStrategy::FillFirst);
    let dead_at = 0i64;
    pool.set_dead_at("d", dead_at);
    pool.prune_dead(dead_at + 24 * 3_600_000 + 1);
    assert!(pool.find("d").is_none());
}

#[test]
fn test_cooldown_is_capped() {
    // S1-R33 — even a provider hint of 8 hours is clamped to MAX_COOLDOWN_MS (5 min).
    let now = 1_000_000i64;
    let mut pool = CredentialPool::new(
        vec![cred("A", CredStatus::Ok, 0)],
        PoolStrategy::FillFirst,
    );
    let eight_hours = now + 8 * 3_600_000;
    pool.mark_exhausted_and_rotate(429, Some(eight_hours), Some("A"), now);
    match pool.find("A").unwrap().status {
        CredStatus::Exhausted { until_ms } => {
            assert!(until_ms <= now + router::pool::MAX_COOLDOWN_MS, "cooldown must be capped");
            assert!(until_ms > now, "cooldown must be in the future");
        }
        _ => panic!("expected Exhausted"),
    }
}

#[test]
fn test_default_429_ttl_is_one_minute() {
    // S1-R32 — a 429 with no hint cools for ~60s, not an hour.
    let now = 1_000_000i64;
    let mut pool =
        CredentialPool::new(vec![cred("A", CredStatus::Ok, 0)], PoolStrategy::FillFirst);
    pool.mark_exhausted_and_rotate(429, None, Some("A"), now);
    match pool.find("A").unwrap().status {
        CredStatus::Exhausted { until_ms } => assert_eq!(until_ms, now + 60_000),
        _ => panic!("expected Exhausted"),
    }
}

#[test]
fn test_mark_ok_heals_a_cooled_credential() {
    // S1-R35 — a successful call clears the credential back to Ok in-memory, and reports the heal
    // (returns true) so the caller persists only on a REAL transition. An already-`Ok` cred is an
    // in-memory no-op → returns false (no DB round-trip — the common per-success case).
    let now = 1_000_000i64;
    let mut pool = CredentialPool::new(
        vec![cred("A", CredStatus::Exhausted { until_ms: now + 60_000 }, 0)],
        PoolStrategy::FillFirst,
    );
    assert!(pool.mark_ok("A"), "an Exhausted→Ok heal returns true (worth persisting)");
    assert_eq!(pool.find("A").unwrap().status, CredStatus::Ok);
    // a SECOND mark_ok on the now-`Ok` cred is a no-op → false (no persist needed).
    assert!(!pool.mark_ok("A"), "an already-Ok cred returns false (no DB round-trip)");
}

#[test]
fn test_mark_ok_does_not_resurrect_a_dead_credential() {
    // S1-R31 — a Dead credential is terminal; a stray success must NOT bring it back into rotation,
    // and mark_ok reports no heal (returns false) so nothing is persisted.
    let mut pool =
        CredentialPool::new(vec![cred("d", CredStatus::Dead, 0)], PoolStrategy::FillFirst);
    assert!(!pool.mark_ok("d"), "a Dead cred is never healed → false (no resurrection)");
    assert_eq!(pool.find("d").unwrap().status, CredStatus::Dead);
}
