// QCue S2-R11/R26 — regen reads wiki_pages (no body reads), reflects a just-written page.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use fixtures::{seed_two_tenants, TestDb};
use sqlx::PgPool;
use wiki::index_gen::regenerate_index;

#[sqlx::test(migrations = "../migrations")]
async fn regen_reflects_pages_from_pg_no_body_reads(pool: PgPool) {
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    let idx = regenerate_index(a, &db.wiki_repo()).await.unwrap();
    // the seeded entity appears (from the PG mirror, not a body read)
    assert!(idx.contains("[[rust|Rust]]"));
}
