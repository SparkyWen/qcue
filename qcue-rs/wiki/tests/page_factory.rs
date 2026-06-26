// QCue S2-R22/R23/R24 — merge=programmatic frontmatter + LLM body (NO_NEW_CONTENT skips); reviewed append-only.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use fixtures::{seed_two_tenants, TestDb};
use sqlx::PgPool;
use wiki::llm::StubWikiLlm;
use wiki::page_factory::{CreateOrUpdate, PageFactory};
use wiki::sandbox::{TenantQuota, TenantSandbox};
use wiki::write_gate::WikiWriteGate;

fn default_sandbox(db: &TestDb, tenant: uuid::Uuid) -> TenantSandbox {
    TenantSandbox { vault_root: db.vault_root(tenant), quota: TenantQuota::default() }
}

#[sqlx::test(migrations = "../migrations")]
async fn no_new_content_skips_write(pool: PgPool) {
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    // the seeded 'rust' entity needs a body on disk for the merge to read; give it one + a char_len.
    let body_ref = db.write_seed_body(a, "rust", "Original Rust body.").await;
    {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(a.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE wiki_pages SET body_ref=$2, char_len=19 WHERE tenant_id=$1 AND slug='rust'")
            .bind(a)
            .bind(&body_ref)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    let llm = StubWikiLlm::scripted(vec!["NO_NEW_CONTENT".into()]);
    let gate = WikiWriteGate::new(db.wiki_repo(), default_sandbox(&db, a));
    let factory = PageFactory::new(&llm, gate, db.wiki_repo());
    let before = db.wiki_repo().existing_pages(a).await.unwrap().iter().find(|p| p.slug == "rust").unwrap().char_len;
    let out = factory
        .create_or_update(
            a,
            CreateOrUpdate {
                name: "Rust".into(),
                r#type: "entity".into(),
                proposed_body: "new para".into(),
                aliases: vec![],
                tags: vec![],
                summary: "s".into(),
                source_id: None,
            },
        )
        .await
        .unwrap();
    assert!(out.skipped_no_new_content);
    let after = db.wiki_repo().existing_pages(a).await.unwrap().iter().find(|p| p.slug == "rust").unwrap().char_len;
    assert_eq!(before, after);
}

#[sqlx::test(migrations = "../migrations")]
async fn reviewed_page_is_append_only(pool: PgPool) {
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    let body_ref = db.write_seed_body(a, "rust", "Original body.").await;
    {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(a.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE wiki_pages SET reviewed=true, body_ref=$2, char_len=14 WHERE tenant_id=$1 AND slug='rust'")
            .bind(a)
            .bind(&body_ref)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    // a reviewed page is NOT LLM-rewritten; the proposed body is appended verbatim under a header.
    let llm = StubWikiLlm::scripted(vec!["Appended fact.".into()]);
    let gate = WikiWriteGate::new(db.wiki_repo(), default_sandbox(&db, a));
    let factory = PageFactory::new(&llm, gate, db.wiki_repo());
    let out = factory
        .create_or_update(
            a,
            CreateOrUpdate {
                name: "Rust".into(),
                r#type: "entity".into(),
                proposed_body: "Appended fact.".into(),
                aliases: vec![],
                tags: vec![],
                summary: "s".into(),
                source_id: None,
            },
        )
        .await
        .unwrap();
    let body = db.gate_read(a, out.page_id).await;
    assert!(body.contains("Original body.")); // preserved
    assert!(body.contains("New Information")); // append-only section header
}
