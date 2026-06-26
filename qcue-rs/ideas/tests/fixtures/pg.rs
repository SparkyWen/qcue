// QCue test fixture (ideas) — seeds a tenant + user + wiki pages over the `#[sqlx::test]`-provided pool
// (M0..M3 migrations applied by the macro). Repos set `app.tenant_id` per-transaction, so the fixture
// seeds parent rows under the tenant GUC (FORCE RLS bites the owner). Tenant isolation is the RLS belt.
//
// `include!`d into each test via `mod fixtures`; carries no inner attributes (the including module
// applies `#[allow(dead_code)]`).
use sqlx::PgPool;
use std::path::PathBuf;
use uuid::Uuid;

pub struct TestDb {
    pub pool: PgPool,
    pub vault: tempfile::TempDir,
}

impl TestDb {
    pub fn new(pool: PgPool) -> Self {
        TestDb { pool, vault: tempfile::tempdir().expect("tempdir") }
    }

    /// The per-tenant vault root `t/<tenant>/u/_` (the propose-write realpath guard resolves under this).
    pub fn vault_root(&self, tenant: Uuid) -> PathBuf {
        let root = self.vault.path().join(format!("t/{tenant}/u/_"));
        std::fs::create_dir_all(root.join("entities")).expect("vault entities");
        root
    }

    /// A `CostGuard` over the shared pool.
    pub fn cost_guard(&self) -> wiki::cost::CostGuard {
        wiki::cost::CostGuard::new(self.pool.clone())
    }

    /// The first user id of tenant `t`.
    pub async fn user_of(&self, t: Uuid) -> Uuid {
        let mut tx = self.pool.begin().await.expect("begin");
        set_tenant(&mut tx, t).await;
        let (uid,): (Uuid,) =
            sqlx::query_as("SELECT id FROM users WHERE tenant_id=$1 ORDER BY created_at LIMIT 1")
                .bind(t)
                .fetch_one(&mut *tx)
                .await
                .expect("user_of");
        tx.commit().await.expect("commit");
        uid
    }

    /// Insert a page of `type`/`slug`/`title`, return its id (for the proposed-merge tests).
    pub async fn insert_page(&self, t: Uuid, ptype: &str, slug: &str, title: &str) -> Uuid {
        let mut tx = self.pool.begin().await.expect("begin");
        set_tenant(&mut tx, t).await;
        let (id,): (Uuid,) = sqlx::query_as(
            "INSERT INTO wiki_pages (tenant_id, type, slug, title, body_ref) \
             VALUES ($1,$2::wiki_page_type,$3,$4,$5) RETURNING id",
        )
        .bind(t)
        .bind(ptype)
        .bind(slug)
        .bind(title)
        .bind(format!("t/{t}/u/_/{ptype}s/{slug}.md"))
        .fetch_one(&mut *tx)
        .await
        .expect("insert page");
        tx.commit().await.expect("commit");
        id
    }

    /// Resolve a page id by (slug, type).
    pub async fn page_id(&self, t: Uuid, slug: &str, ptype: &str) -> Uuid {
        let mut tx = self.pool.begin().await.expect("begin");
        set_tenant(&mut tx, t).await;
        let (id,): (Uuid,) = sqlx::query_as(
            "SELECT id FROM wiki_pages WHERE tenant_id=$1 AND slug=$2 AND type=$3::wiki_page_type",
        )
        .bind(t)
        .bind(slug)
        .bind(ptype)
        .fetch_one(&mut *tx)
        .await
        .expect("page_id");
        tx.commit().await.expect("commit");
        id
    }

    /// Seed the tenant ledger to the daily cap so the next `check_before_call` refuses (cost-abort test).
    pub async fn max_out_cost(&self, t: Uuid) {
        let mut tx = self.pool.begin().await.expect("begin");
        set_tenant(&mut tx, t).await;
        sqlx::query(
            "INSERT INTO cost_ledger (tenant_id, scope, user_id, day, cost_micros) \
             VALUES ($1,'tenant',NULL,current_date,5000000)",
        )
        .bind(t)
        .execute(&mut *tx)
        .await
        .expect("max out cost");
        tx.commit().await.expect("commit");
    }
}

async fn set_tenant(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, tenant: Uuid) {
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut **tx)
        .await
        .expect("set tenant guc");
}

/// Seed a single tenant with a user + one `rust` entity page; return its id.
pub async fn seed_tenant(db: &TestDb) -> Uuid {
    let t = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, slug, display_name, namespace) VALUES ($1,$2,$2,$3)")
        .bind(t)
        .bind(format!("t-{t}"))
        .bind(format!("t/{t}"))
        .execute(&db.pool)
        .await
        .expect("insert tenant");
    let mut tx = db.pool.begin().await.expect("begin");
    set_tenant(&mut tx, t).await;
    sqlx::query("INSERT INTO users (id, tenant_id, email) VALUES ($1,$2,$3)")
        .bind(Uuid::now_v7())
        .bind(t)
        .bind(format!("u-{t}@x.test"))
        .execute(&mut *tx)
        .await
        .expect("insert user");
    sqlx::query(
        "INSERT INTO wiki_pages (tenant_id, type, slug, title, body_ref) \
         VALUES ($1,'entity','rust','Rust',$2)",
    )
    .bind(t)
    .bind(format!("t/{t}/u/_/entities/rust.md"))
    .execute(&mut *tx)
    .await
    .expect("insert page");
    tx.commit().await.expect("commit");
    t
}
