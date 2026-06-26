//! QCue S2 — the `ingest` JobHandler: the capture→wiki-page loop. The worker claims a `kind='ingest'`
//! job carrying `{idea_id}`, this handler loads the idea (tenant-scoped, RLS) and runs the wiki
//! conversation-ingest 6-stage pipeline through the single `WikiLlm` seam + the single write-gate, then
//! returns the `IngestReport` as the job result. The `WikiLlm` is injectable (`RouterWikiLlm` in
//! production; a scripted `StubWikiLlm` in tests) so the loop is exercisable keyless.
pub mod router_llm;
pub mod routes;

use crate::jobs::worker::{JobContext, JobHandler};
use async_trait::async_trait;
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use store::ideas_repo::IdeasRepo;
use store::wiki_repo::WikiRepo;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wiki::cost::CostGuard;
use wiki::ingest::{IdeaInput, IngestDeps, IngestJob};
use wiki::llm::WikiLlm;

pub use router_llm::RouterWikiLlm;

/// The ingest service: holds the shared pool, the per-tenant vault root resolver, and the LLM seam.
pub struct IngestHandler {
    pool: PgPool,
    vault_root_base: PathBuf,
    llm: Arc<dyn WikiLlm>,
    language: String,
}

impl IngestHandler {
    pub fn new(pool: PgPool, vault_root_base: PathBuf, llm: Arc<dyn WikiLlm>) -> Self {
        Self { pool, vault_root_base, llm, language: "en".into() }
    }

    /// The per-tenant vault root `<base>/t/<tenant>/u/_` (the write-gate isolates bodies under this).
    fn vault_root(&self, tenant: Uuid) -> PathBuf {
        let root = self.vault_root_base.join(format!("t/{tenant}/u/_"));
        let _ = std::fs::create_dir_all(&root);
        root
    }

    /// Load `(user_id, body, origin)` for the idea under the tenant GUC (FORCE RLS).
    async fn load_idea(&self, tenant: Uuid, idea_id: Uuid) -> anyhow::Result<(Uuid, String, String)> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        let row: (Uuid, String, String) =
            sqlx::query_as("SELECT user_id, body, origin FROM ideas WHERE tenant_id=$1 AND id=$2")
                .bind(tenant)
                .bind(idea_id)
                .fetch_one(&mut *tx)
                .await?;
        tx.commit().await?;
        Ok(row)
    }
}

#[async_trait]
impl JobHandler for IngestHandler {
    async fn handle(&self, job: &JobContext) -> Result<serde_json::Value, String> {
        let tenant = job.tenant_id;
        let idea_id = job
            .payload
            .get("idea_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(|| "ingest payload missing idea_id".to_string())?;
        let (user, body, origin) =
            self.load_idea(tenant, idea_id).await.map_err(|e| e.to_string())?;
        let deps = IngestDeps::new(
            self.llm.as_ref(),
            self.vault_root(tenant),
            WikiRepo::new(self.pool.clone()),
            IdeasRepo::new(self.pool.clone()),
            CostGuard::new(self.pool.clone()),
        )
        .with_language(&self.language)
        // The source-page `form` tags are inherited from the capture origin (S2-R3), never LLM-derived.
        .with_source_tags(vec![origin.clone()]);
        let idea = IdeaInput { id: idea_id, body, origin };
        let report = IngestJob::run(tenant, user, idea, &deps, CancellationToken::new())
            .await
            .map_err(|e| e.to_string())?;
        serde_json::to_value(&report).map_err(|e| e.to_string())
    }
}
