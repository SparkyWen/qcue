// QCue A-R4..R8 — lock-as-clock: absent→epoch0; acquire returns prior clock; two workers one wins;
// rollback rewinds. Faithful to Claude's `.consolidate-lock` mtime semantics (App. A §2.2): acquire
// advances the clock, a live unexpired lease blocks a second worker, rollback rewinds it.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use chrono::{TimeZone, Utc};
use fixtures::{seed_two_tenants, TestDb};
use sqlx::PgPool;
use wiki::dream::lock::{ConsolidationLock, PgConsolidationLock};

/// Seed the per-tenant `wiki_consolidation` row under the tenant GUC (FORCE RLS bites the owner too).
async fn seed_consolidation_row(db: &TestDb, t: uuid::Uuid) {
    let mut tx = db.pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
        .bind(t.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO wiki_consolidation (tenant_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(t)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

#[sqlx::test(migrations = "../migrations")]
async fn clock_acquire_rollback_semantics(pool: PgPool) {
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    seed_consolidation_row(&db, a).await;
    let lock = PgConsolidationLock::new(db.pool.clone());

    // A-R4 — absent/epoch clock (a fresh row has NULL last_consolidated_at → epoch 0).
    let c0 = lock.read_clock(a).await.unwrap();
    assert_eq!(c0, Utc.timestamp_opt(0, 0).unwrap());

    // A-R5 — acquire returns Some(prior); a second acquire is blocked (None) by the live lease.
    let prior = lock.try_acquire(a, "w1").await.unwrap();
    assert!(prior.is_some());
    let second = lock.try_acquire(a, "w2").await.unwrap();
    assert!(second.is_none()); // live unexpired holder → blocked

    // A-R5 — the acquire advanced the clock to ~now (the row analog of writing the PID sets mtime=now).
    let advanced = lock.read_clock(a).await.unwrap();
    assert!(advanced > c0);

    // A-R8 — rollback rewinds the clock to prior and frees the lease (scan-throttle is the backoff).
    lock.rollback(a, prior.unwrap()).await.unwrap();
    let after = lock.read_clock(a).await.unwrap();
    assert_eq!(after, prior.unwrap());

    // …and the lease is freed, so a fresh worker can acquire again.
    let reacquired = lock.try_acquire(a, "w3").await.unwrap();
    assert!(reacquired.is_some());
}

#[sqlx::test(migrations = "../migrations")]
async fn release_keeps_advanced_clock(pool: PgPool) {
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    seed_consolidation_row(&db, a).await;
    let lock = PgConsolidationLock::new(db.pool.clone());
    // A-R7 — on success the clock stays advanced; release frees the lease but does NOT rewind.
    let prior = lock.try_acquire(a, "w1").await.unwrap();
    assert!(prior.is_some());
    let advanced = lock.read_clock(a).await.unwrap();
    lock.release(a).await.unwrap();
    let after = lock.read_clock(a).await.unwrap();
    assert_eq!(after, advanced); // clock unchanged by release
                                 // lease is free → a new worker can acquire.
    assert!(lock.try_acquire(a, "w2").await.unwrap().is_some());
}
