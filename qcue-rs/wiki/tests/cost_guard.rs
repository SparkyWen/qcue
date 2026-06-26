// QCue S2-R64/R19 — ceiling checked BEFORE a call; usage deduped by request_id (never summed across blocks).
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use fixtures::{seed_two_tenants, TestDb};
use protocol::CanonicalUsage;
use sqlx::PgPool;
use wiki::cost::CostGuard;

#[sqlx::test(migrations = "../migrations")]
async fn refuses_when_ceiling_hit_and_dedups_by_request(pool: PgPool) {
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    let user = db.user_of(a).await;
    // set the day's tenant spend to the $5 cap (default daily_cost_cap_micros = 5_000_000).
    {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(a.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO cost_ledger (tenant_id, scope, user_id, day, cost_micros) \
             VALUES ($1,'tenant',NULL,current_date,5000000)",
        )
        .bind(a)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let guard = CostGuard::new(db.pool.clone());
    assert!(guard.check_before_call(a, user).await.is_err()); // over ceiling → refuse, no call

    // dedup: accrue the SAME request_id twice → ledger increments once.
    let usage = CanonicalUsage { input: 100, output: 50, cache_read: 0, cache_write: 0, reasoning: 10 };
    guard.accrue(a, user, "req-1", &usage, 1000).await.unwrap();
    guard.accrue(a, user, "req-1", &usage, 1000).await.unwrap(); // same request — no double-bill

    let total: i64 = {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(a.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let v: (i64,) = sqlx::query_as(
            "SELECT cost_micros FROM cost_ledger WHERE tenant_id=$1 AND scope='tenant' AND user_id IS NULL AND day=current_date",
        )
        .bind(a)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        v.0
    };
    assert_eq!(total, 5_001_000); // cap + exactly one 1000-micro accrual
}
