//! QCue S2 — the `dream` JobHandler: the Auto-Dream consolidation loop. The S3 dream-scheduler cron
//! (a separate milestone) enqueues a `kind='dream'` job carrying `{user_id, current_session}`; this
//! handler builds the `DreamScheduler` (the lock-as-clock + the cheapest-gate-first ladder) over the
//! tenant and drives the harness-driven `DreamAgent` (the read-only fork) through it.
//!
//! Faithful wiring (App. A §2): the agent reaches the model ONLY through `RouterWikiLlm` (the single
//! `WikiLlm` seam), the session gate uses the LIVE authoritative `IdeasRepo::captures_since` count, and
//! writes are PROPOSED (candidates→confirm). The job result is the "Improved N pages" report (the
//! files-touched feed). A gated-out tick returns `{ dreamed: false }` with zero provider calls.
//!
//! The S3-finish milestone adds two submodules: `routes` (the `POST /v1/dream/run` manual-run surface —
//! enqueues a `kind='dream'` job; the `GET /v1/dream/{job}/stream` SSE mirror is mounted in
//! `wire::routes`), and `scheduler` (the per-tenant Tokio-interval cron that, gated by `DREAM_ENABLED`,
//! calls `DreamScheduler::dream_due(tenant)` and enqueues a `kind='dream'` job when due).
pub mod routes;
pub mod scheduler;

use crate::ingest::RouterWikiLlm;
use crate::jobs::worker::{JobContext, JobHandler};
use async_trait::async_trait;
use ideas::dream::agent::DreamAgent;
use router::turn::Harness;
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use store::ideas_repo::IdeasRepo;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wiki::cost::CostGuard;
use wiki::dream::lock::PgConsolidationLock;
use wiki::dream::scheduler::DreamScheduler;
use wiki::llm::WikiLlm;

/// The dream service: holds the shared pool, the per-tenant vault-root resolver, and a factory for the
/// per-tenant `WikiLlm` (the router harness routes per (tenant, provider) in production; a stub-backed
/// harness keeps the loop keyless in tests).
pub struct DreamHandler {
    pool: PgPool,
    vault_root_base: PathBuf,
    llm: Arc<dyn WikiLlm>,
    /// Optional per-job progress channel: the Dream-detail screen subscribes to
    /// `GET /v1/dream/{job}/stream` and receives `dream_started/progress/completed/failed` keyed by the
    /// job id (App. A §4). `None` keeps the handler self-contained for the keyless e2e tests.
    progress: Option<crate::wire::hub::StreamHub>,
}

impl DreamHandler {
    pub fn new(pool: PgPool, vault_root_base: PathBuf, llm: Arc<dyn WikiLlm>) -> Self {
        Self { pool, vault_root_base, llm, progress: None }
    }

    /// Wire the per-job progress channel so the SSE Dream-detail stream receives lifecycle events.
    pub fn with_progress(mut self, hub: crate::wire::hub::StreamHub) -> Self {
        self.progress = Some(hub);
        self
    }

    /// Build a dream handler whose model seam is a stub-backed router harness (keyless/networkless).
    pub fn with_stub_harness(pool: PgPool, vault_root_base: PathBuf, reply: &str) -> Self {
        Self::new(pool, vault_root_base, Arc::new(RouterWikiLlm::new(Harness::with_stub(
            router::stub::StubProvider::new(router::stub::StubScript::text(reply)),
        ))))
    }

    /// Build a dream handler whose model seam is the live (env-gated) real-dispatch harness. The Dream
    /// fork advertises the read-only + `propose_*` tool surface so the model authors propose calls (the
    /// agent collects their target paths into `files_touched`). `QCUE_STUB_LLM=1` keeps it keyless.
    pub fn live(
        pool: PgPool,
        vault_root_base: PathBuf,
        kms: Arc<dyn secrets::Kms + Send + Sync>,
        stub_reply: &str,
    ) -> Self {
        let tools = ideas::recall::tool_policy::build_tool_policy(true, false).tools; // Dream: offline (no web).
        Self::new(
            pool.clone(),
            vault_root_base,
            Arc::new(RouterWikiLlm::live_with_tools(pool, kms, tools, stub_reply)),
        )
    }

    /// Publish one dream lifecycle SSE event (`dream_started/progress/completed/failed`) on the per-job
    /// channel, if a progress hub is wired. A no-op otherwise.
    fn emit(&self, job_id: Uuid, event: &str, payload: serde_json::Value) {
        if let Some(hub) = &self.progress {
            let seq = hub.next_seq(job_id);
            hub.publish(app_server_protocol::RuntimeEventEnvelope {
                schema_version: 1,
                thread_id: job_id,
                turn_id: None,
                seq,
                event: event.to_string(),
                payload,
            });
        }
    }

    /// The per-tenant vault root `<base>/t/<tenant>/u/_` (the propose-write realpath guard resolves
    /// targets under this; the write-gate isolates bodies under it).
    fn vault_root(&self, tenant: Uuid) -> PathBuf {
        let root = self.vault_root_base.join(format!("t/{tenant}/u/_"));
        let _ = std::fs::create_dir_all(root.join("entities"));
        root
    }
}

#[async_trait]
impl JobHandler for DreamHandler {
    async fn handle(&self, job: &JobContext) -> Result<serde_json::Value, String> {
        let tenant = job.tenant_id;
        // The cron enqueues `{user_id, current_session}`. `user_id` scopes the per-user cost ledger;
        // `current_session` is excluded from the session gate (the in-flight session is always recent).
        let user = job
            .payload
            .get("user_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(|| "dream payload missing user_id".to_string())?;
        let current_session = job
            .payload
            .get("current_session")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(Uuid::now_v7);

        // dream_started — the Dream-detail screen flips to "running" (App. A §4 / §3.4 dream taxonomy).
        self.emit(job.id, "dream_started", serde_json::json!({ "tenant": tenant.to_string() }));

        let cost = CostGuard::new(self.pool.clone());
        let agent = DreamAgent::new(self.llm.as_ref(), self.vault_root(tenant), &cost);
        let sched = DreamScheduler::new(
            PgConsolidationLock::new(self.pool.clone()),
            IdeasRepo::new(self.pool.clone()),
            agent,
        );
        match sched
            .try_dream(tenant, user, current_session, CancellationToken::new())
            .await
        {
            Ok(Some(outcome)) => {
                // completed — the "Improved N pages" report (the files-touched feed, A-R15).
                self.emit(
                    job.id,
                    "dream_completed",
                    serde_json::json!({
                        "improved_pages": outcome.files_touched.len(),
                        "files_touched": outcome.files_touched,
                        "turns": outcome.turns,
                    }),
                );
                Ok(serde_json::json!({
                    "dreamed": true,
                    "improved_pages": outcome.files_touched.len(),
                    "files_touched": outcome.files_touched,
                    "turns": outcome.turns,
                }))
            }
            Ok(None) => {
                // a gate stopped it (no-op, no $ burned) — completed with nothing changed.
                self.emit(job.id, "dream_completed", serde_json::json!({ "dreamed": false }));
                Ok(serde_json::json!({ "dreamed": false }))
            }
            Err(e) => {
                // failure already rewound the clock inside try_dream; surface it to the detail screen.
                self.emit(job.id, "dream_failed", serde_json::json!({ "error": e.to_string() }));
                Err(e.to_string())
            }
        }
    }
}
