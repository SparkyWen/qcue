// QCue S2-R26/R29 — read the index FIRST (empty handled, not invented); synthesis enforces wiki-only +
// [[links]] only + a mandatory ## References block; citations are parsed from References.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use fixtures::{seed_two_tenants, TestDb};
use sqlx::PgPool;
use store::wiki_repo::WikiRepo;
use wiki::llm::StubWikiLlm;
use wiki::query::{recall_query, Answer, QueryEngine, RecallSink};

/// An in-memory sink proving the `recall_query` SSE seam is wired (the S3 handler will stream instead).
#[derive(Default)]
struct CollectSink {
    answers: Vec<String>,
}
#[async_trait::async_trait]
impl RecallSink for CollectSink {
    async fn emit_answer(&mut self, answer: &Answer) -> anyhow::Result<()> {
        self.answers.push(answer.text.clone());
        Ok(())
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn synthesis_enforces_wiki_only_links_and_references(pool: PgPool) {
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    // the seeded `rust` page exists in the PG mirror (catalog); a body on disk lets the engine load it.
    db.write_seed_body(a, "rust", "# Rust\nA systems language.").await;
    // stub returns a synthesized answer already containing [[links]] + a ## References block.
    let llm = StubWikiLlm::scripted(vec![
        "Rust is a systems language [[rust]].\n\n## References\n[[rust|Rust]] — the entity page".into(),
    ]);
    let eng = QueryEngine::new(&llm, WikiRepo::new(db.tenant_pool()), db.vault_root(a));
    let ans = eng.answer(a, "What is Rust?").await.unwrap();
    assert!(ans.text.contains("## References")); // mandatory references (S2-R29)
    assert!(ans.text.contains("[[rust]]")); // [[links]] only
    assert!(!ans.text.contains("http://") && !ans.text.contains("](")); // no html/markdown links
    assert!(!ans.citations.is_empty()); // citations parsed from References
    assert_eq!(ans.citations[0].rel_path, "rust.md");
}

#[sqlx::test(migrations = "../migrations")]
async fn empty_wiki_is_handled_not_invented(pool: PgPool) {
    let db = TestDb::new(pool);
    // a fresh tenant with NO wiki pages (seed only the tenant + a user, no page).
    let t = fresh_tenant_no_pages(&db).await;
    let llm = StubWikiLlm::scripted(vec!["The wiki is empty; nothing to answer.".into()]);
    let eng = QueryEngine::new(&llm, WikiRepo::new(db.tenant_pool()), db.vault_root(t));
    let ans = eng.answer(t, "anything").await.unwrap();
    assert!(ans.text.to_lowercase().contains("empty"));
    // the index-first read produced the "(wiki is empty)" sentinel in the system prefix, not an
    // invented page list — the stub recorded the last system prefix it was handed.
    assert!(llm.last_system().contains("wiki is empty"));
}

#[sqlx::test(migrations = "../migrations")]
async fn recall_query_seam_emits_to_sink(pool: PgPool) {
    // the clean seam the S3 recall-SSE handler will call (non-streaming today; streaming in S3-finish).
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    let llm = StubWikiLlm::scripted(vec!["Rust [[rust]].\n\n## References\n[[rust]] — page".into()]);
    let eng = QueryEngine::new(&llm, WikiRepo::new(db.tenant_pool()), db.vault_root(a));
    let mut sink = CollectSink::default();
    recall_query(a, "What is Rust?", &eng, &mut sink).await.unwrap();
    assert_eq!(sink.answers.len(), 1);
    assert!(sink.answers[0].contains("## References"));
}

/// Seed a tenant + a user but NO wiki pages (the empty-wiki branch).
async fn fresh_tenant_no_pages(db: &TestDb) -> uuid::Uuid {
    let t = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, slug, display_name, namespace) VALUES ($1,$2,$2,$3)")
        .bind(t)
        .bind(format!("t-{t}"))
        .bind(format!("t/{t}"))
        .execute(&db.pool)
        .await
        .expect("insert tenant");
    let mut tx = db.pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(t.to_string())
        .execute(&mut *tx)
        .await
        .expect("set guc");
    sqlx::query("INSERT INTO users (id, tenant_id, email) VALUES ($1,$2,$3)")
        .bind(uuid::Uuid::now_v7())
        .bind(t)
        .bind(format!("u-{t}@x.test"))
        .execute(&mut *tx)
        .await
        .expect("insert user");
    tx.commit().await.expect("commit");
    t
}
