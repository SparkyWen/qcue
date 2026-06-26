// QCue DIG-R1/DIG-R4 — edited-idea re-ingest updates the source_id-linked page IN PLACE (no duplicate,
// no orphan) and stamps last_ingested_at on success.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use fixtures::{seed_two_tenants, TestDb};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wiki::ingest::{IdeaInput, IngestDeps, IngestJob};
use wiki::llm::StubWikiLlm;

async fn last_ingested_is_set(db: &TestDb, t: Uuid, idea_id: Uuid) -> bool {
    let mut tx = db.pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id',$1,true)").bind(t.to_string()).execute(&mut *tx).await.unwrap();
    let row: (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT last_ingested_at FROM ideas WHERE id=$1").bind(idea_id).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    row.0.is_some()
}

fn extract(title: &str) -> String {
    format!(
        r#"{{"source_title":"{title}","summary":"s","entities":[],"concepts":[],"contradictions":[],"related_pages":[],"key_points":[]}}"#
    )
}

#[sqlx::test(migrations = "../migrations")]
async fn reingest_of_edited_idea_updates_same_page_no_orphan(pool: PgPool) {
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    let user = db.user_of(a).await;
    let idea = db.insert_idea(a, "Notes about Tokio async runtime").await;
    let idea_id = idea.id;

    // First ingest: extraction names the source "Tokio notes" → slug "tokio-notes".
    let llm1 = StubWikiLlm::scripted(vec![
        r#"{"fully_redundant":false}"#.into(),
        extract("Tokio notes"),
        "summary v1".into(),
    ]);
    let deps1 = IngestDeps::new(&llm1, db.vault_root(a), db.wiki_repo(), db.ideas_repo(), db.cost_guard());
    IngestJob::run(a, user, idea, &deps1, CancellationToken::new()).await.unwrap();

    // capture the source page id + that last_ingested_at was stamped.
    let pages = db.wiki_repo().existing_pages(a).await.unwrap();
    let src1 = pages.iter().find(|p| p.r#type == "source").unwrap().clone();
    assert_eq!(src1.slug, "tokio-notes");
    assert!(last_ingested_is_set(&db, a, idea_id).await, "last_ingested_at stamped on first ingest");

    // Second ingest of the SAME idea (an edit), where extraction now names it "Tokio runtime" → a
    // DIFFERENT derived slug. Because the page is source_id-linked, the run must REUSE the prior slug.
    let edited = IdeaInput { id: idea_id, body: "Notes about Tokio async runtime, expanded".into(), origin: "capture".into() };
    let llm2 = StubWikiLlm::scripted(vec![
        r#"{"fully_redundant":false}"#.into(),
        extract("Tokio runtime"), // would slug to "tokio-runtime" if not reusing the prior slug
        "summary v2".into(),
    ]);
    let deps2 = IngestDeps::new(&llm2, db.vault_root(a), db.wiki_repo(), db.ideas_repo(), db.cost_guard());
    IngestJob::run(a, user, edited, &deps2, CancellationToken::new()).await.unwrap();

    let after = db.wiki_repo().existing_pages(a).await.unwrap();
    let sources: Vec<_> = after.iter().filter(|p| p.r#type == "source").collect();
    assert_eq!(sources.len(), 1, "exactly one source page (no duplicate/orphan): {sources:?}");
    assert_eq!(sources[0].id, src1.id, "the SAME page id was updated in place");
    assert_eq!(sources[0].slug, "tokio-notes", "the prior slug was reused, not a new 'tokio-runtime'");
}
