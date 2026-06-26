// SBX-R5: a write past the per-tenant page/byte cap is rejected; under the cap it succeeds.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use wiki::sandbox::{TenantQuota, TenantSandbox};
use wiki::write_gate::{PageWrite, WikiWriteGate};
use store::wiki_repo::WikiRepo;
use uuid::Uuid;

fn pw(slug: &str, body: &str) -> PageWrite {
    PageWrite {
        r#type: "concept".into(),
        slug: slug.into(),
        title: slug.into(),
        aliases: vec![],
        tags: vec![],
        summary: String::new(),
        source_ids: vec![],
        body: body.into(),
        llm_created: None,
        llm_reviewed: None,
    }
}

/// Seed the minimum tenant+user rows required by FORCE-RLS policies.
/// `wiki_pages` has `tenant_id` FK → `tenants`; `tenants` has no RLS.
async fn seed_tenant(pool: &sqlx::PgPool, tenant: Uuid) {
    sqlx::query(
        "INSERT INTO tenants (id, slug, display_name, namespace) VALUES ($1,$2,$2,$3) \
         ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(format!("t-{tenant}"))
    .bind(format!("t/{tenant}"))
    .execute(pool)
    .await
    .expect("insert tenant");

    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant guc");
    sqlx::query("INSERT INTO users (id, tenant_id, email) VALUES ($1,$2,$3)")
        .bind(Uuid::now_v7())
        .bind(tenant)
        .bind(format!("u-{tenant}@x.test"))
        .execute(&mut *tx)
        .await
        .expect("insert user");
    tx.commit().await.expect("commit");
}

#[sqlx::test(migrations = "../migrations")]
async fn write_is_rejected_past_the_page_cap(pool: sqlx::PgPool) {
    let tenant = Uuid::now_v7();
    seed_tenant(&pool, tenant).await;

    let tmp = tempfile::tempdir().unwrap();
    let sandbox = TenantSandbox {
        vault_root: tmp.path().join("t/x/u/y"),
        quota: TenantQuota { max_pages: 1, max_bytes: 1_000_000 },
    };
    std::fs::create_dir_all(&sandbox.vault_root).unwrap();
    let gate = WikiWriteGate::new(WikiRepo::new(pool.clone()), sandbox);

    gate.write_page(tenant, pw("first", "ok")).await.expect("first write under cap");
    let err = gate.write_page(tenant, pw("second", "nope")).await;
    assert!(err.is_err(), "second write exceeds max_pages=1 → should be rejected");
    let msg = err.unwrap_err().to_string();
    assert!(msg.contains("quota exceeded"), "error should mention quota: {msg}");
}

#[sqlx::test(migrations = "../migrations")]
async fn write_is_rejected_past_the_byte_cap(pool: sqlx::PgPool) {
    let tenant = Uuid::now_v7();
    seed_tenant(&pool, tenant).await;

    let tmp = tempfile::tempdir().unwrap();
    // byte cap of 5 — smaller than any realistic write.
    let sandbox = TenantSandbox {
        vault_root: tmp.path().join("t/x/u/z"),
        quota: TenantQuota { max_pages: 1_000, max_bytes: 5 },
    };
    std::fs::create_dir_all(&sandbox.vault_root).unwrap();
    let gate = WikiWriteGate::new(WikiRepo::new(pool.clone()), sandbox);

    // First write has a 10-char body → 10 > cap 5 → rejected.
    let err = gate.write_page(tenant, pw("any", "0123456789")).await;
    assert!(err.is_err(), "body exceeds max_bytes=5 → should be rejected");
    let msg = err.unwrap_err().to_string();
    assert!(msg.contains("quota exceeded"), "error should mention quota: {msg}");
}
