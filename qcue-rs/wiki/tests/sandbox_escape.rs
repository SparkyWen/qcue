// SBX-R7 — adversarial: an LLM-authored escaping slug/path can never write outside the tenant vault.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use store::wiki_repo::WikiRepo;
use uuid::Uuid;
use wiki::sandbox::{TenantQuota, TenantSandbox};
use wiki::write_gate::{PageWrite, WikiWriteGate};

fn evil_pw(slug: &str) -> PageWrite {
    PageWrite {
        r#type: "concept".into(),
        slug: slug.into(),
        title: "x".into(),
        aliases: vec![],
        tags: vec![],
        summary: String::new(),
        source_ids: vec![],
        body: "pwned".into(),
        llm_created: None,
        llm_reviewed: None,
    }
}

/// Seed the minimum tenant+user rows required by FORCE-RLS policies.
/// Copied verbatim from `wiki/tests/quota.rs` — the single authoritative helper.
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
async fn escaping_slug_cannot_write_outside_the_vault(pool: sqlx::PgPool) {
    let tenant = Uuid::now_v7();
    seed_tenant(&pool, tenant).await;

    let tmp = tempfile::tempdir().unwrap();
    let sandbox = TenantSandbox {
        vault_root: tmp.path().join("t/x/u/y"),
        quota: TenantQuota::default(),
    };
    std::fs::create_dir_all(&sandbox.vault_root).unwrap();
    let gate = WikiWriteGate::new(WikiRepo::new(pool.clone()), sandbox);

    for slug in [
        "../../../tmp/evil",
        "../../escape",
        "/etc/cron.d/evil",
        "a/../../b",
    ] {
        let result = gate.write_page(tenant, evil_pw(slug)).await;
        assert!(
            result.is_err(),
            "escaping slug {slug:?} must be rejected, but write succeeded"
        );
    }

    // Nothing was written outside the vault root.
    assert!(
        !tmp.path().join("tmp/evil.md").exists(),
        "escape target tmp/evil.md must not exist"
    );
    assert!(
        !tmp.path().join("escape.md").exists(),
        "escape target escape.md must not exist"
    );
    // The /etc path must not have been touched (if /etc/cron.d/evil.md were created it would exist).
    // We cannot assert a system path, but we can assert that no file named evil.md escaped root.
    assert!(
        !tmp.path().join("evil.md").exists(),
        "evil.md must not appear at the tmpdir root"
    );
}
