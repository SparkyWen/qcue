// QCue S2-R35..R40 — scanners are pure SQL over wiki_links/wiki_pages; no LLM, no body reads.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[allow(dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use fixtures::{seed_lint_fixtures, seed_two_tenants, TestDb};
use store::wiki_repo::WikiRepo;
use wiki::lint::scanners::Scanners;

#[sqlx::test(migrations = "../migrations")]
async fn scanners_detect_dead_orphan_empty_alias_tag(pool: sqlx::PgPool) {
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    // seed: a page with a dead link, an orphan, an empty page (char_len<50), an entity with no aliases,
    // and a tag outside the vocabulary.
    seed_lint_fixtures(&db, a).await;
    let sc = Scanners::new(WikiRepo::new(db.tenant_pool()));
    assert!(!sc.dead_links(a).await.unwrap().is_empty()); // S2-R35
    assert!(!sc.orphans(a).await.unwrap().is_empty()); // S2-R36
    assert!(!sc.empty_pages(a).await.unwrap().is_empty()); // S2-R37 (char_len)
    assert!(!sc.missing_aliases(a).await.unwrap().is_empty()); // S2-R38
    assert!(!sc.tag_violations(a, &["theory".into()]).await.unwrap().is_empty()); // S2-R39
}
