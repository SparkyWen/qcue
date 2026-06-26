// QCue S2-R49/R4/R5/R6/R37 — the one write site: sanitizes links, parses the link-graph,
// system-stamps frontmatter + char_len. Runs against the real qcue Postgres (M0..M3) under RLS.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[allow(dead_code)]
mod fixtures {
    include!("fixtures/pg.rs");
}
use fixtures::{seed_two_tenants, TestDb};
use store::wiki_repo::WikiRepo;
use wiki::sandbox::{TenantQuota, TenantSandbox};
use wiki::write_gate::{PageWrite, WikiWriteGate};

fn default_sandbox(db: &TestDb, tenant: uuid::Uuid) -> TenantSandbox {
    TenantSandbox { vault_root: db.vault_root(tenant), quota: TenantQuota::default() }
}

#[sqlx::test(migrations = "../migrations")]
async fn single_write_gate_sanitizes_links_and_indexes_graph(pool: sqlx::PgPool) {
    let db = TestDb::new(pool);
    let (a, _b) = seed_two_tenants(&db).await;
    // seed 'rust' exists (entity); also seed 'x'/'foo' as entity/concept so two of the three links resolve.
    let gate = WikiWriteGate::new(WikiRepo::new(db.tenant_pool()), default_sandbox(&db, a));
    // pre-seed link targets X and Foo so they are NOT dead.
    let _x = gate
        .write_page(
            a,
            PageWrite {
                r#type: "entity".into(),
                slug: "x".into(),
                title: "X".into(),
                aliases: vec![],
                tags: vec![],
                summary: String::new(),
                source_ids: vec![],
                body: "X page body that is reasonably long for substance.".into(),
                llm_created: None,
                llm_reviewed: None,
            },
        )
        .await
        .unwrap();
    let _foo = gate
        .write_page(
            a,
            PageWrite {
                r#type: "concept".into(),
                slug: "foo".into(),
                title: "Foo".into(),
                aliases: vec![],
                tags: vec![],
                summary: String::new(),
                source_ids: vec![],
                body: "Foo page body that is reasonably long for substance.".into(),
                llm_created: None,
                llm_reviewed: None,
            },
        )
        .await
        .unwrap();

    let id = gate
        .write_page(
            a,
            PageWrite {
                r#type: "concept".into(),
                slug: "graphs".into(),
                title: "Graphs".into(),
                aliases: vec![],
                tags: vec!["theory".into()],
                summary: "About graphs".into(),
                source_ids: vec![],
                // polluted links + an LLM-supplied created/reviewed that MUST be stripped.
                body: "See [[entities/X|entities/X]] and [[concepts/conceptsFoo|Foo]] and [[graphs]]."
                    .into(),
                llm_created: Some("1999-01-01T00:00:00Z".into()),
                llm_reviewed: Some(true),
            },
        )
        .await
        .unwrap();

    // body persisted sanitized.
    let body = gate.read_body(a, id).await.unwrap();
    assert!(body.contains("[[X]]") && body.contains("[[Foo]]"));
    assert!(!body.contains("entities/X|entities/X"));
    // link-graph upserted: 3 links; [[graphs]] resolves to self, [[X]] and [[Foo]] resolve to seeded pages.
    let rows = WikiRepo::new(db.tenant_pool()).links_of(a, id).await.unwrap();
    assert_eq!(rows.len(), 3);
    let graphs_self = rows.iter().find(|(slug, _)| slug == "graphs").unwrap();
    assert!(graphs_self.1.is_some()); // self-link resolved (target_page_id set)
    // char_len system-set from the real sanitized body length (S2-R37); reviewed NOT taken from LLM.
    let page = WikiRepo::new(db.tenant_pool()).page(a, id).await.unwrap();
    assert!(page.char_len > 0);
    assert_eq!(page.char_len as usize, body.chars().count()); // exactly the sanitized body length
    assert!(!page.reviewed); // LLM-supplied reviewed:true ignored on a fresh page (S2-R6)
}

// SYNC-D6/D2 (Task 5): write_page sets content_hash = sha-256(sanitized body) and bumps sync_version
// on each materialized write. A fresh page starts at version 1; a second write bumps to 2.
#[sqlx::test(migrations = "../migrations")]
async fn write_page_sets_content_hash_and_bumps_sync_version(pool: sqlx::PgPool) {
    use sha2::{Digest, Sha256};
    let db = TestDb::new(pool);
    let (a, _b) = seed_two_tenants(&db).await;
    let gate = WikiWriteGate::new(WikiRepo::new(db.tenant_pool()), default_sandbox(&db, a));

    let mk = |body: &str| PageWrite {
        r#type: "concept".into(),
        slug: "hashing".into(),
        title: "Hashing".into(),
        aliases: vec![],
        tags: vec![],
        summary: String::new(),
        source_ids: vec![],
        body: body.into(),
        llm_created: None,
        llm_reviewed: None,
    };

    // first write → sync_version = 1, content_hash = sha256(sanitized body).
    let id = gate.write_page(a, mk("First body, reasonably substantive.")).await.unwrap();
    let sanitized1 = gate.read_body(a, id).await.unwrap();
    let want_hash1 = {
        let mut h = Sha256::new();
        h.update(sanitized1.as_bytes());
        h.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    let (hash1, ver1) = read_hash_version(&db, a, id).await;
    assert_eq!(hash1.as_deref(), Some(want_hash1.as_str()), "content_hash = sha256 of sanitized body");
    assert_eq!(ver1, 1, "fresh page starts at sync_version 1");

    // second write of a DIFFERENT body → hash changes, sync_version bumps to 2.
    gate.write_page(a, mk("Second body — different content entirely here.")).await.unwrap();
    let sanitized2 = gate.read_body(a, id).await.unwrap();
    let want_hash2 = {
        let mut h = Sha256::new();
        h.update(sanitized2.as_bytes());
        h.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    let (hash2, ver2) = read_hash_version(&db, a, id).await;
    assert_eq!(hash2.as_deref(), Some(want_hash2.as_str()));
    assert_ne!(hash1, hash2, "content_hash tracks the body");
    assert_eq!(ver2, ver1 + 1, "sync_version bumped by 1 on the second write");
}

/// Read (content_hash, sync_version) for a page under the tenant GUC.
async fn read_hash_version(db: &TestDb, tenant: uuid::Uuid, id: uuid::Uuid) -> (Option<String>, i64) {
    let mut tx = db.pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant guc");
    let row: (Option<String>, i64) =
        sqlx::query_as("SELECT content_hash, sync_version FROM wiki_pages WHERE tenant_id=$1 AND id=$2")
            .bind(tenant)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .expect("read hash/version");
    tx.commit().await.expect("commit");
    row
}
