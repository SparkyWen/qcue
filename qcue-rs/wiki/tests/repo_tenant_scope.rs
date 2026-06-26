// QCue S2-R21/S2-R62 — existing-pages lookup is SQL + tenant-scoped (RLS). Uses the M0..M3 migrations.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[allow(dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use fixtures::{seed_two_tenants, TestDb};
use store::wiki_repo::WikiRepo;

#[sqlx::test(migrations = "../migrations")]
async fn existing_pages_lookup_is_sql_tenant_scoped(pool: sqlx::PgPool) {
    let db = TestDb::new(pool);
    let (tenant_a, _tenant_b) = seed_two_tenants(&db).await; // each gets one entity page
    let repo = WikiRepo::new(db.tenant_pool());
    let pages = repo.existing_pages(tenant_a).await.unwrap(); // tenant-scoped + GUC-bound
    assert_eq!(pages.len(), 1); // only A's page; B's filtered by RLS
    assert_eq!(pages[0].r#type, "entity");
    // a forgotten WHERE still cannot leak B (RLS belt): query all under A's GUC, assert still 1.
    let all = repo.all_pages_raw(tenant_a).await.unwrap();
    assert_eq!(all.len(), 1);
}
