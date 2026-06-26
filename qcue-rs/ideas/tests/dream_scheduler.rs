// QCue A-R3/A-R8/A-R9/A-R10/A-R11/A-R20 — the cheapest-gate-first ladder wired end-to-end: the
// `wiki::DreamScheduler` drives the `ideas::DreamAgent` (the `DreamRunner` seam) over the real
// lock-as-clock + the live authoritative `IdeasRepo::captures_since` count.
//
//   - gated-out (too few captures) → zero provider calls + one indexed single-row read (A-R3/A-R10);
//   - cost-ceiling mid-dream → the agent errs BEFORE any call, the scheduler ROLLS BACK the clock to
//     the prior value (A-R8/A-R20) and the scan-throttle is the backoff;
//   - DREAM_ENABLED=false → the whole ladder is gated off (pitfall #16), zero calls.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use chrono::{DateTime, Utc};
use fixtures::{seed_tenant, TestDb};
use ideas::dream::agent::DreamAgent;
use sqlx::PgPool;
use store::ideas_repo::IdeasRepo;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wiki::dream::lock::PgConsolidationLock;
use wiki::dream::scheduler::DreamScheduler;
use wiki::llm::StubWikiLlm;

/// Seed the per-tenant `wiki_consolidation` row (under the tenant GUC).
async fn seed_consolidation(db: &TestDb, t: Uuid) {
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

/// Force the time-gate open by rewinding the clock 48h into the past.
async fn open_time_gate(db: &TestDb, t: Uuid) {
    let mut tx = db.pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
        .bind(t.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE wiki_consolidation SET last_consolidated_at = now() - interval '48 hours' WHERE tenant_id=$1",
    )
    .bind(t)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// Insert `n` captures so the session gate (minSessions=5) can pass.
async fn seed_captures(db: &TestDb, t: Uuid, user: Uuid, n: usize) {
    for _ in 0..n {
        let id = Uuid::now_v7();
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(t.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
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
        tx.commit().await.unwrap();
    }
}

async fn read_clock(db: &TestDb, t: Uuid) -> DateTime<Utc> {
    let mut tx = db.pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
        .bind(t.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let r: (Option<DateTime<Utc>>,) =
        sqlx::query_as("SELECT last_consolidated_at FROM wiki_consolidation WHERE tenant_id=$1")
            .bind(t)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    r.0.unwrap()
}

#[sqlx::test(migrations = "../migrations")]
async fn gated_out_when_too_few_captures_zero_provider_calls(pool: PgPool) {
    let db = TestDb::new(pool);
    let t = seed_tenant(&db).await;
    let user = db.user_of(t).await;
    seed_consolidation(&db, t).await;
    open_time_gate(&db, t).await; // pass time gate, but ZERO captures → session gate stops

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
    assert!(out.is_none()); // session gate stopped it
    assert_eq!(llm.call_count(), 0); // A-R3 — zero provider calls past a failed gate
}

#[sqlx::test(migrations = "../migrations")]
async fn all_gates_pass_drives_agent_and_advances_clock(pool: PgPool) {
    let db = TestDb::new(pool);
    let t = seed_tenant(&db).await;
    let user = db.user_of(t).await;
    seed_consolidation(&db, t).await;
    open_time_gate(&db, t).await;
    seed_captures(&db, t, user, 5).await; // session gate passes (≥5)

    let llm = StubWikiLlm::scripted(vec!["Consolidated 0 pages; nothing changed.".into()]);
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
    assert!(out.is_some()); // all gates passed → the agent ran
    assert_eq!(llm.call_count(), 1);
    // A-R7 — on success the clock STAYS advanced (the acquire set it to ~now; release kept it). So the
    // time gate now fails (< 24h since), i.e. the clock is no longer 48h in the past.
    let after = read_clock(&db, t).await;
    assert!((Utc::now() - after).num_hours() < 1);
}

#[sqlx::test(migrations = "../migrations")]
async fn session_gate_excludes_current_session(pool: PgPool) {
    let db = TestDb::new(pool);
    let t = seed_tenant(&db).await;
    let user = db.user_of(t).await;
    seed_consolidation(&db, t).await;
    open_time_gate(&db, t).await;
    // 5 captures all tagged with the SAME ingest_job_id = the "current session" → all excluded → 0 count.
    let current = Uuid::now_v7();
    for _ in 0..5 {
        let id = Uuid::now_v7();
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(t.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO ideas (id, tenant_id, user_id, kind, body, log_ref, origin, ingest_state, ingest_job_id) \
             VALUES ($1,$2,$3,'text','x',$4,'capture','pending',$5)",
        )
        .bind(id)
        .bind(t)
        .bind(user)
        .bind(format!("captures/{id}.jsonl"))
        .bind(current)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let llm = StubWikiLlm::counting();
    let cost = db.cost_guard();
    let agent = DreamAgent::new(&llm, db.vault_root(t), &cost);
    let sched = DreamScheduler::new(
        PgConsolidationLock::new(db.pool.clone()),
        IdeasRepo::new(db.pool.clone()),
        agent,
    );
    // A-R10 — the current session's captures are excluded, so the live count is 0 < minSessions → stop.
    let out = sched
        .try_dream(t, user, current, CancellationToken::new())
        .await
        .unwrap();
    assert!(out.is_none());
    assert_eq!(llm.call_count(), 0);
}

#[sqlx::test(migrations = "../migrations")]
async fn cost_ceiling_mid_dream_rolls_back_clock_zero_calls(pool: PgPool) {
    let db = TestDb::new(pool);
    let t = seed_tenant(&db).await;
    let user = db.user_of(t).await;
    seed_consolidation(&db, t).await;
    open_time_gate(&db, t).await;
    seed_captures(&db, t, user, 5).await; // session gate passes
    db.max_out_cost(t).await; // but a $0-remaining ledger

    let prior = read_clock(&db, t).await; // the pre-acquire clock (48h ago)
    let llm = StubWikiLlm::counting();
    let cost = db.cost_guard();
    let agent = DreamAgent::new(&llm, db.vault_root(t), &cost);
    let sched = DreamScheduler::new(
        PgConsolidationLock::new(db.pool.clone()),
        IdeasRepo::new(db.pool.clone()),
        agent,
    );
    let res = sched
        .try_dream(t, user, Uuid::now_v7(), CancellationToken::new())
        .await;
    assert!(res.is_err()); // cost ceiling aborts the agent
    assert_eq!(llm.call_count(), 0); // A-R20 — zero provider calls

    // A-R8 — the acquire advanced the clock; the rollback REWOUND it to prior (within a tolerance for
    // the µs-level timestamp round-trip — they must be equal to the second the test set).
    let after = read_clock(&db, t).await;
    assert_eq!(after, prior);
}

