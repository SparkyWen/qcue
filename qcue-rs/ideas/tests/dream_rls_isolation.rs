// QCue S2-R62 / A-R5/A-R19 — RLS cross-tenant isolation for the Dream slice's tables: the lock-as-clock
// (`wiki_consolidation`) and the candidates→confirm gate (`approvals`). Tenant A acquiring its lock or
// proposing a merge MUST never be visible to / collide with tenant B (the load-bearing belt is RLS, not
// an app-side filter — pitfall #14). FORCE RLS bites even the table owner; an unset GUC reads zero rows.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use fixtures::{seed_tenant, TestDb};
use sqlx::PgPool;
use uuid::Uuid;
use wiki::approvals::{route_destructive, DestructiveOp};
use wiki::dream::lock::{ConsolidationLock, PgConsolidationLock};

async fn count_consolidation(db: &TestDb, t: Uuid) -> i64 {
    let mut tx = db.pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
        .bind(t.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let r: (i64,) = sqlx::query_as("SELECT count(*) FROM wiki_consolidation")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    r.0
}

async fn count_approvals(db: &TestDb, t: Uuid) -> i64 {
    let mut tx = db.pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
        .bind(t.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let r: (i64,) = sqlx::query_as("SELECT count(*) FROM approvals")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    r.0
}

#[sqlx::test(migrations = "../migrations")]
async fn dream_tables_are_tenant_isolated(pool: PgPool) {
    let db = TestDb::new(pool);
    let a = seed_tenant(&db).await;
    let b = seed_tenant(&db).await;
    let user_a = db.user_of(a).await;
    // Seed A's consolidation row + acquire A's lock (writes A's clock + lease).
    {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(a.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO wiki_consolidation (tenant_id) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(a)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    let lock = PgConsolidationLock::new(db.pool.clone());
    assert!(lock.try_acquire(a, "wa").await.unwrap().is_some());

    // A proposes a merge (folds A's dup into A's `rust` entity) → one PENDING approval for A.
    let dup = db.insert_page(a, "concept", "rust-lang", "Rust Lang").await;
    let into = db.page_id(a, "rust", "entity").await;
    route_destructive(&db.pool, a, user_a, "dream", DestructiveOp::WikiMerge { from: dup, into })
        .await
        .unwrap();

    // A sees its own consolidation row + approval; B sees NEITHER (RLS isolation, pitfall #14).
    assert_eq!(count_consolidation(&db, a).await, 1);
    assert_eq!(count_consolidation(&db, b).await, 0);
    assert_eq!(count_approvals(&db, a).await, 1);
    assert_eq!(count_approvals(&db, b).await, 0);

    // B's lock acquire is independent of A's (a separate row keyed by B's tenant_id) — B is not blocked.
    {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(b.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO wiki_consolidation (tenant_id) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(b)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    assert!(lock.try_acquire(b, "wb").await.unwrap().is_some()); // B acquires despite A holding A's lease
}
