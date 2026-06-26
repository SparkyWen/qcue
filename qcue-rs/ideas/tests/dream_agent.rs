// QCue S2-R57/R59 / A-R12/R15/R16/R19/R20 — the harness-driven Dream agent: it uses the SHARED recall
// tool policy + propose_* (A-R40), checks cost BEFORE the provider call (A-R20), proposes merges through
// the candidates→confirm gate (A-R19, lands as a PENDING `approvals` row + reversible soft-delete), and
// realpath-guards proposed writes to the tenant root (A-R12). No network; no main-transcript writes.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use chrono::{TimeZone, Utc};
use fixtures::{seed_tenant, TestDb};
use ideas::dream::agent::DreamAgent;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use wiki::llm::StubWikiLlm;

#[sqlx::test(migrations = "../migrations")]
async fn dream_uses_recall_policy_plus_propose_and_costs_before_call(pool: PgPool) {
    let db = TestDb::new(pool);
    let a = seed_tenant(&db).await;
    let user = db.user_of(a).await;
    let llm = StubWikiLlm::scripted(vec!["Consolidated 0 pages; nothing changed.".into()]);
    let cost = db.cost_guard();
    let agent = DreamAgent::new(&llm, db.vault_root(a), &cost);

    // A-R40 — the policy is the shared builder, differing only by propose_* (read-only otherwise).
    assert!(agent.tool_policy().allow_propose);
    assert!(agent.tool_policy().network_off);
    assert!(agent.tool_policy().root_confined);
    assert!(agent.tool_policy().tools.iter().any(|t| t.name == "recall_search"));
    assert!(agent.tool_policy().tools.iter().any(|t| t.name == "read_page"));
    assert!(agent.tool_policy().tools.iter().any(|t| t.name == "propose_edit"));

    let out = agent
        .run(a, user, Utc.timestamp_opt(0, 0).unwrap(), CancellationToken::new())
        .await
        .unwrap();
    assert!(out.turns >= 1);
    assert_eq!(llm.call_count(), 1); // exactly one provider call drove the dream
}

#[sqlx::test(migrations = "../migrations")]
async fn dream_proposed_merge_lands_as_pending_approval_not_canonical(pool: PgPool) {
    let db = TestDb::new(pool);
    let a = seed_tenant(&db).await;
    let user = db.user_of(a).await;
    // a duplicate page to fold into the seeded `rust` entity.
    let dup = db.insert_page(a, "concept", "rust-lang", "Rust Lang").await;
    let into = db.page_id(a, "rust", "entity").await;
    let llm = StubWikiLlm::scripted(vec!["merge".into()]);
    let cost = db.cost_guard();
    let agent = DreamAgent::new(&llm, db.vault_root(a), &cost);

    // A-R19 — the proposed merge routes through the §8.3 gate (approvals + soft-delete), NOT canonical.
    agent.propose_merge(a, user, dup, into).await.unwrap();

    let appr: (i64,) = {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(a.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let r = sqlx::query_as(
            "SELECT count(*) FROM approvals WHERE tenant_id=$1 AND action='wiki_merge' AND status='pending'",
        )
        .bind(a)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        r
    };
    assert_eq!(appr.0, 1); // proposed, not canonical (D13, A-R19)

    // the merge source is reversibly soft-deleted; the merge TARGET (canonical) is untouched.
    let (dup_del, into_del): (Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>) = {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(a.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let d: (Option<chrono::DateTime<chrono::Utc>>,) =
            sqlx::query_as("SELECT deleted_at FROM wiki_pages WHERE id=$1")
                .bind(dup)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        let i: (Option<chrono::DateTime<chrono::Utc>>,) =
            sqlx::query_as("SELECT deleted_at FROM wiki_pages WHERE id=$1")
                .bind(into)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        tx.commit().await.unwrap();
        (d.0, i.0)
    };
    assert!(dup_del.is_some()); // reversible soft-delete of the merge source
    assert!(into_del.is_none()); // canonical merge target unchanged
}

#[sqlx::test(migrations = "../migrations")]
async fn cost_ceiling_aborts_before_any_provider_call(pool: PgPool) {
    let db = TestDb::new(pool);
    let a = seed_tenant(&db).await;
    let user = db.user_of(a).await;
    db.max_out_cost(a).await; // $0 remaining
    let llm = StubWikiLlm::counting();
    let cost = db.cost_guard();
    let agent = DreamAgent::new(&llm, db.vault_root(a), &cost);
    // A-R20 — the cost cap is checked BEFORE the call; a $0 ledger aborts with zero provider calls.
    let res = agent.run(a, user, Utc.timestamp_opt(0, 0).unwrap(), CancellationToken::new()).await;
    assert!(res.is_err());
    assert_eq!(llm.call_count(), 0);
}

#[sqlx::test(migrations = "../migrations")]
async fn propose_write_outside_root_is_denied(pool: PgPool) {
    let db = TestDb::new(pool);
    let a = seed_tenant(&db).await;
    let user = db.user_of(a).await;
    let llm = StubWikiLlm::scripted(vec![]);
    let cost = db.cost_guard();
    let agent = DreamAgent::new(&llm, db.vault_root(a), &cost);
    // A-R12 — a propose_write targeting outside the tenant root (traversal) is rejected by the guard;
    // an in-root .md is allowed.
    assert!(agent
        .propose_write(a, user, "../../B/u/Y/entities/x.md", "evil")
        .await
        .is_err());
    assert!(agent.propose_write(a, user, "entities/ok.md", "fine").await.is_ok());
}
