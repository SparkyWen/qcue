// QCue test fixture — seeds two tenants + per-tenant vault roots over the `#[sqlx::test]`-provided
// pool (M0..M3 migrations applied by the macro). The repos set `app.tenant_id` per-transaction, so the
// fixture seeds parent rows as the DB owner (under FORCE RLS the GUC must be set per write) and hands
// the same pool to the repo under test. Tenant isolation is proven by the RLS belt, not a second pool.
//
// NOTE: this file is `include!`d into each test via `mod fixtures`, so it carries no inner attributes;
// the including test module applies `#[allow(dead_code)]` (not every test uses every helper).
use sqlx::PgPool;
use std::path::PathBuf;
use uuid::Uuid;

/// A wrapper over the injected pool + a throwaway vault directory for body writes.
pub struct TestDb {
    pub pool: PgPool,
    pub vault: tempfile::TempDir,
}

impl TestDb {
    pub fn new(pool: PgPool) -> Self {
        TestDb { pool, vault: tempfile::tempdir().expect("tempdir") }
    }

    /// The same pool — repos scope every statement via a per-tx `app.tenant_id` GUC, so no second
    /// connection/role is needed to exercise RLS (FORCE RLS bites even the table owner).
    pub fn tenant_pool(&self) -> PgPool {
        self.pool.clone()
    }

    /// The per-tenant vault root `t/<tenant>/u/_`. The write-gate resolves bodies under this.
    pub fn vault_root(&self, tenant: Uuid) -> PathBuf {
        let root = self.vault.path().join(format!("t/{tenant}/u/_"));
        std::fs::create_dir_all(&root).expect("vault root");
        root
    }

    /// A `WikiRepo` over the shared pool (the repos scope every statement by per-tx GUC).
    pub fn wiki_repo(&self) -> store::wiki_repo::WikiRepo {
        store::wiki_repo::WikiRepo::new(self.pool.clone())
    }

    /// An `IdeasRepo` over the shared pool.
    pub fn ideas_repo(&self) -> store::ideas_repo::IdeasRepo {
        store::ideas_repo::IdeasRepo::new(self.pool.clone())
    }

    /// A `CostGuard` over the shared pool.
    pub fn cost_guard(&self) -> wiki::cost::CostGuard {
        wiki::cost::CostGuard::new(self.pool.clone())
    }

    /// The first user id of tenant `t` (seeded by `seed_two_tenants`).
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

    /// Insert a `text` idea for tenant `t` and return its id (the ingest job input).
    pub async fn insert_idea(&self, t: Uuid, body: &str) -> wiki::ingest::IdeaInput {
        self.insert_idea_origin(t, body, "capture").await
    }

    /// Insert a clip idea carrying inherited origin tags (used as the SOURCE-page tag set).
    pub async fn insert_idea_with_origin_tags(
        &self,
        t: Uuid,
        body: &str,
        _tags: &[&str],
    ) -> wiki::ingest::IdeaInput {
        self.insert_idea_origin(t, body, "web").await
    }

    async fn insert_idea_origin(&self, t: Uuid, body: &str, origin: &str) -> wiki::ingest::IdeaInput {
        let uid = self.user_of(t).await;
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await.expect("begin");
        set_tenant(&mut tx, t).await;
        sqlx::query(
            "INSERT INTO ideas (id, tenant_id, user_id, kind, body, log_ref, origin, ingest_state) \
             VALUES ($1,$2,$3,'text',$4,$5,$6,'pending')",
        )
        .bind(id)
        .bind(t)
        .bind(uid)
        .bind(body)
        .bind(format!("captures/{id}.jsonl"))
        .bind(origin)
        .execute(&mut *tx)
        .await
        .expect("insert idea");
        tx.commit().await.expect("commit");
        wiki::ingest::IdeaInput { id, body: body.to_string(), origin: origin.to_string() }
    }

    /// Write a seed body file at the page's body_ref and return the absolute path (for reviewed-page
    /// tests where the merge reads the existing body off disk).
    pub async fn write_seed_body(&self, tenant: Uuid, slug: &str, body: &str) -> String {
        let root = self.vault_root(tenant);
        let dir = root.join("entities");
        std::fs::create_dir_all(&dir).expect("seed dir");
        let path = dir.join(format!("{slug}.md"));
        std::fs::write(&path, body).expect("seed body");
        path.to_string_lossy().to_string()
    }

    /// Read a page body via a fresh write-gate (the only body reader callers use).
    pub async fn gate_read(&self, tenant: Uuid, id: Uuid) -> String {
        let sandbox = wiki::sandbox::TenantSandbox {
            vault_root: self.vault_root(tenant),
            quota: wiki::sandbox::TenantQuota::default(),
        };
        let gate = wiki::write_gate::WikiWriteGate::new(self.wiki_repo(), sandbox);
        gate.read_body(tenant, id).await.expect("read body")
    }
}

/// Set the request tenant GUC for the duration of one transaction.
async fn set_tenant(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, tenant: Uuid) {
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut **tx)
        .await
        .expect("set tenant guc");
}

/// Seed two tenants, each with a user + one entity page. Returns (tenant_a, tenant_b).
pub async fn seed_two_tenants(db: &TestDb) -> (Uuid, Uuid) {
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    for t in [a, b] {
        // `tenants` is the global root (no RLS); `users`/`wiki_pages` are FORCE-RLS, so the user + page
        // inserts run inside one transaction with `app.tenant_id` set (WITH CHECK passes).
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
            "INSERT INTO wiki_pages (tenant_id, type, slug, title, body_ref) VALUES ($1,'entity','rust','Rust',$2)",
        )
        .bind(t)
        .bind(format!("t/{t}/u/_/entities/rust.md"))
        .execute(&mut *tx)
        .await
        .expect("insert page");
        tx.commit().await.expect("commit");
    }
    (a, b)
}

/// Seed lint-trigger fixtures for tenant `t`: a page with a dead link, an orphan, an empty page, an
/// entity with no aliases, and a page carrying an out-of-vocabulary tag. All under the tenant GUC.
pub async fn seed_lint_fixtures(db: &TestDb, t: Uuid) {
    let mut tx = db.pool.begin().await.expect("begin");
    set_tenant(&mut tx, t).await;

    // A substantive page that links to a non-existent target → a DEAD link, and is itself an ORPHAN
    // (nothing links to it), and carries an out-of-vocab tag (a tag-violation hit). char_len ≥ 50 so it
    // is NOT an empty-page hit.
    let (src,): (Uuid,) = sqlx::query_as(
        "INSERT INTO wiki_pages (tenant_id, type, slug, title, aliases, tags, char_len, body_ref) \
         VALUES ($1,'concept','graph-theory','Graph Theory','{theory}','{badtag}',500,$2) RETURNING id",
    )
    .bind(t)
    .bind(format!("t/{t}/u/_/concepts/graph-theory.md"))
    .fetch_one(&mut *tx)
    .await
    .expect("insert concept page");

    // dead link: target_page_id NULL (no such page 'nowhere')
    sqlx::query(
        "INSERT INTO wiki_links (tenant_id, src_page_id, target_slug, target_page_id) VALUES ($1,$2,'nowhere',NULL)",
    )
    .bind(t)
    .bind(src)
    .execute(&mut *tx)
    .await
    .expect("insert dead link");

    // an empty page (char_len < 50) that is also a missing-aliases hit (entity, no aliases).
    sqlx::query(
        "INSERT INTO wiki_pages (tenant_id, type, slug, title, aliases, char_len, body_ref) \
         VALUES ($1,'entity','stub','Stub','{}',10,$2)",
    )
    .bind(t)
    .bind(format!("t/{t}/u/_/entities/stub.md"))
    .execute(&mut *tx)
    .await
    .expect("insert empty page");

    tx.commit().await.expect("commit");
}
