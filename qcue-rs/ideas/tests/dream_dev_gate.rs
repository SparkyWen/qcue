// QCue A-R20 / pitfall #16 — `DREAM_ENABLED=false` gates the whole ladder off (cron never burns real
// provider $ in dev). Isolated in its OWN test binary (a separate process) so the env-var mutation can't
// race the parallel `dream_scheduler` tests that need `dream_enabled()==true`.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use fixtures::{seed_tenant, TestDb};
use ideas::dream::agent::DreamAgent;
use sqlx::PgPool;
use store::ideas_repo::IdeasRepo;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wiki::dream::lock::PgConsolidationLock;
use wiki::dream::scheduler::DreamScheduler;
use wiki::llm::StubWikiLlm;

#[sqlx::test(migrations = "../migrations")]
async fn workers_gated_off_in_dev(pool: PgPool) {
    let db = TestDb::new(pool);
    let t = seed_tenant(&db).await;
    let user = db.user_of(t).await;
    // seed the consolidation row + open the time gate + 5 captures, so ONLY the enabled-gate can stop it.
    {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(t.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO wiki_consolidation (tenant_id, last_consolidated_at) \
             VALUES ($1, now() - interval '48 hours') ON CONFLICT DO NOTHING",
        )
        .bind(t)
        .execute(&mut *tx)
        .await
        .unwrap();
        for _ in 0..5 {
            let id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO ideas (id, tenant_id, user_id, kind, body, log_ref, origin, ingest_state) \
                 VALUES ($1,$2,$3,'text','capture body',$4,'capture','pending')",
            )
            .bind(id)
            .bind(t)
            .bind(user)
            .bind(format!("captures/{id}.jsonl"))
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        tx.commit().await.unwrap();
    }

    // SAFETY: this is the only test in this binary (own process) — no env-var race.
    unsafe { std::env::set_var("DREAM_ENABLED", "false") };
    let llm = StubWikiLlm::counting();
    let cost = db.cost_guard();
    let agent = DreamAgent::new(&llm, db.vault_root(t), &cost);
    let sched = DreamScheduler::new(
        PgConsolidationLock::new(db.pool.clone()),
        IdeasRepo::new(db.pool.clone()),
        agent,
    );
    let out = sched
        .try_dream(t, user, Uuid::now_v7(), CancellationToken::new())
        .await
        .unwrap();
    unsafe { std::env::remove_var("DREAM_ENABLED") };
    assert!(out.is_none()); // gated off (enabled gate)
    assert_eq!(llm.call_count(), 0);
}
