// QCue A-R6 — a stale (lease_expires < now) lock is reclaimable regardless of holder (the 1h
// HOLDER_STALE_MS guard; a dead/expired holder must not wedge the clock forever).
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use fixtures::{seed_two_tenants, TestDb};
use sqlx::PgPool;
use wiki::dream::lock::{ConsolidationLock, PgConsolidationLock};

#[sqlx::test(migrations = "../migrations")]
async fn stale_lease_is_reclaimable(pool: PgPool) {
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    // Seed a row held by a "live-looking" but EXPIRED holder (lease_expires 2h in the past).
    {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(a.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO wiki_consolidation (tenant_id, holder, lease_expires) \
             VALUES ($1,'dead-worker', now() - interval '2 hours') \
             ON CONFLICT (tenant_id) DO UPDATE SET holder='dead-worker', lease_expires=now()-interval '2 hours'",
        )
        .bind(a)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let lock = PgConsolidationLock::new(db.pool.clone());
    // reclaimed despite a non-NULL holder, because the lease has expired (A-R6).
    assert!(lock.try_acquire(a, "fresh-worker").await.unwrap().is_some());
}
