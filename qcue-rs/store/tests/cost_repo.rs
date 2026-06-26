#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R55 / B-R20 — cost_ledger pre-call read + accrue against the per-tenant/day ceiling (D17).
// Uses real Postgres via the single workspace migrations dir (M0 + M1 + M2).
use store::cost_repo::{CostRepo, CostUsage};
use uuid::Uuid;

#[sqlx::test(migrations = "../migrations")]
async fn test_accrue_and_read_today_per_scope(pool: sqlx::PgPool) {
    let repo = CostRepo::new(pool.clone());
    let tenant = Uuid::now_v7();
    let user = Uuid::now_v7();
    // a fresh day starts at zero for both scopes.
    let (t0, u0) = repo.read_today(tenant, user).await.unwrap();
    assert_eq!(t0, 0);
    assert_eq!(u0, 0);
    // accrue 1_000_000 micros ($1) of spend with all five token kinds present.
    repo.accrue(
        tenant,
        user,
        CostUsage { input: 100, output: 20, cache_read: 5, cache_write: 3, reasoning: 7 },
        1_000_000,
        "openai",
    )
    .await
    .unwrap();
    let (t1, u1) = repo.read_today(tenant, user).await.unwrap();
    assert_eq!(t1, 1_000_000, "tenant-scope micros accrued");
    assert_eq!(u1, 1_000_000, "user-scope micros accrued");
    // a second accrual sums into the same day rows (upsert, not a new row).
    repo.accrue(
        tenant,
        user,
        CostUsage { input: 10, output: 2, cache_read: 0, cache_write: 0, reasoning: 1 },
        500_000,
        "openai",
    )
    .await
    .unwrap();
    let (t2, _) = repo.read_today(tenant, user).await.unwrap();
    assert_eq!(t2, 1_500_000);
}

#[sqlx::test(migrations = "../migrations")]
async fn test_ceiling_blocks_over_cap(pool: sqlx::PgPool) {
    let repo = CostRepo::new(pool.clone());
    let tenant = Uuid::now_v7();
    let user = Uuid::now_v7();
    // seed parents with a tiny $0.000001 (1 micro) tenant cap so any prior spend trips it.
    repo.seed_caps(tenant, user, 1, 1).await.unwrap();
    // before any spend, a zero ledger is UNDER even a 1-micro cap.
    assert!(repo.check_ceiling(tenant, user).await.unwrap().is_ok());
    // accrue 2 micros → over both the 1-micro tenant and user caps.
    repo.accrue(
        tenant,
        user,
        CostUsage { input: 1, output: 0, cache_read: 0, cache_write: 0, reasoning: 0 },
        2,
        "openai",
    )
    .await
    .unwrap();
    let decision = repo.check_ceiling(tenant, user).await.unwrap();
    assert!(decision.is_err(), "over-ceiling must refuse before any call (B-R20, D17)");
}
