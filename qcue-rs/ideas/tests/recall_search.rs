// QCue S2-R31 / A-R21..R25 — recall_search registered + routes CJK/Latin against real Postgres FTS +
// is tenant-scoped (RLS) + carries bookends + a safe citation. The model authors the pattern; the
// harness only routes it (tsvector for Latin, pg_trgm/ILIKE for CJK) and runs it.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("../../wiki/tests/fixtures/pg.rs");
}
use fixtures::{seed_two_tenants, TestDb};
use ideas::recall::route::SearchMode;
use ideas::recall::search_tool::{recall_search_tool, run_recall_search, RecallArgs, RecallMode};
use sqlx::PgPool;
use store::search_repo::SearchRepo;

#[test]
fn tool_is_registered_with_expected_name() {
    let spec = recall_search_tool();
    assert_eq!(spec.name, "recall_search"); // S2-R31 — first-class harness tool name
}

#[sqlx::test(migrations = "../migrations")]
async fn routes_and_is_tenant_scoped(pool: PgPool) {
    let db = TestDb::new(pool);
    let (a, b) = seed_two_tenants(&db).await;
    // seed a Latin idea in BOTH tenants; a search as A must never see B's rows (RLS, pitfall #14).
    db.insert_idea(a, "deployment runbook for the database migration").await;
    db.insert_idea(b, "deployment runbook for the database migration").await;
    let repo = SearchRepo::new(db.tenant_pool());

    // Latin query → tsvector (Postgres FTS), and only tenant A's rows are visible.
    let (mode, hits) = run_recall_search(
        a,
        &repo,
        RecallArgs { pattern: "database migration".into(), mode: RecallMode::Discovery, current_session: None },
    )
    .await
    .unwrap();
    assert_eq!(mode, SearchMode::Tsvector);
    assert!(hits.iter().all(|h| h.tenant_scoped_ok)); // only A's rows; B filtered by RLS
    assert!(!hits.is_empty());
    // bookends are attached (goal/conclusion from the lineage), plus a conservative citation.
    assert!(hits[0].goal.is_some());
    assert!(hits[0].citation.is_some());

    // CJK query (≥3-char phrase) → trigram path; seeded CJK capture is found.
    db.insert_idea(a, "数据库迁移 步骤 笔记").await;
    let (mode, hits) = run_recall_search(
        a,
        &repo,
        RecallArgs { pattern: "数据库迁移".into(), mode: RecallMode::Discovery, current_session: None },
    )
    .await
    .unwrap();
    assert_eq!(mode, SearchMode::Trigram);
    assert!(!hits.is_empty());
    assert!(hits[0].citation.is_some());

    // a 2-char CJK token still routes trigram; a single CJK char routes ILIKE (the Like path).
    let (mode2, _) = run_recall_search(
        a,
        &repo,
        RecallArgs { pattern: "步骤".into(), mode: RecallMode::Discovery, current_session: None },
    )
    .await
    .unwrap();
    assert_eq!(mode2, SearchMode::Trigram);
    let (mode3, _) = run_recall_search(
        a,
        &repo,
        RecallArgs { pattern: "学".into(), mode: RecallMode::Discovery, current_session: None },
    )
    .await
    .unwrap();
    assert_eq!(mode3, SearchMode::Like);
}
