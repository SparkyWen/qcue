// QCue S2-R3/R7..R12/R16/R19/R51 — the conversation-ingest 6-stage pipeline (the primary capture
// path). Persist-first happens at capture (ideas crate); here we run stages 1-6 over a persisted idea:
//
//   1. ensureStructure        — implicit: the write-gate creates the typed folders on first write.
//   2. dedup gate FIRST       — ask the WikiLlm `fully_redundant?` vs the materialized index; if so,
//                               skip every write and set ingest_state='skipped_redundant' (S2-R7).
//   3. source-analyze         — single extraction call (conversation mode; S2-R8) over the WikiLlm.
//   4. semantic source page   — a semantic-slug `sources/<slug>.md` summary; tags are INHERITED from the
//                               capture origin, never LLM-derived (S2-R3).
//   5. entity/concept CRUD    — bounded-concurrency, per-item retry, failures isolated (one bad item
//                               does not abort the batch; S2-R10).
//   6. related/contradictions + regen index + ingest_state='ingested' — reversible/idempotent (re-run safe).
//
// Untrusted capture content is fenced (`<untrusted_source>`) + reserved-tag-escaped before it enters the
// message TAIL (RKM §7); the cache-safe system prefix carries only stable instruction text (pitfall #2).
// Every WikiLlm call is preceded by a cost-cap-before-call read (S2-R19/R64) so an exhausted ledger
// aborts BEFORE any provider call.
use crate::cost::CostGuard;
use crate::extract::analyzer::{AnalyzeMode, SourceAnalyzer};
use crate::index_gen::regenerate_index;
use crate::json_hardening::parse_json_response;
use crate::llm::{SystemBlocks, WikiLlm, WikiReq};
use crate::page_factory::{CreateOrUpdate, PageFactory};
use crate::types::IngestReport;
use crate::write_gate::{PageWrite, WikiWriteGate};
use fence::fence_untrusted;
use futures_util::stream::{FuturesUnordered, StreamExt};
use protocol::{Message, Role};
use slugify::slugify;
use std::path::PathBuf;
use store::ideas_repo::IdeasRepo;
use store::wiki_repo::WikiRepo;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct IdeaInput {
    pub id: Uuid,
    pub body: String,
    pub origin: String,
}

pub struct IngestDeps<'a, L: WikiLlm + ?Sized> {
    pub llm: &'a L,
    pub vault_root: PathBuf,
    pub repo: WikiRepo,
    pub ideas: IdeasRepo,
    pub cost: CostGuard,
    pub concurrency: usize,
    pub language: String,
    /// Tags inherited from the capturing origin (the source `form` taxonomy) — set on the SOURCE page;
    /// NEVER LLM-derived (S2-R3, anti-pollution).
    pub source_tags: Vec<String>,
}

impl<'a, L: WikiLlm + ?Sized> IngestDeps<'a, L> {
    pub fn new(
        llm: &'a L,
        vault_root: PathBuf,
        repo: WikiRepo,
        ideas: IdeasRepo,
        cost: CostGuard,
    ) -> Self {
        Self {
            llm,
            vault_root,
            repo,
            ideas,
            cost,
            concurrency: 4,
            language: "en".into(),
            source_tags: vec![],
        }
    }
    pub fn with_source_tags(mut self, tags: Vec<String>) -> Self {
        self.source_tags = tags;
        self
    }
    pub fn with_concurrency(mut self, n: usize) -> Self {
        self.concurrency = n.max(1);
        self
    }
    pub fn with_language(mut self, lang: &str) -> Self {
        self.language = lang.into();
        self
    }
}

pub struct IngestJob;
impl IngestJob {
    pub async fn run<L: WikiLlm + ?Sized>(
        tenant: Uuid,
        user: Uuid,
        idea: IdeaInput,
        deps: &IngestDeps<'_, L>,
        cancel: CancellationToken,
    ) -> anyhow::Result<IngestReport> {
        let mut report = IngestReport::default();
        // Stage 1 — ensureStructure is implicit (folders created by the write-gate on first write).
        // Stage 2 — dedup gate FIRST: fully_redundant? vs the materialized index (S2-R7).
        deps.cost.check_before_call(tenant, user).await?;
        let index = regenerate_index(tenant, &deps.repo).await?;
        if Self::is_redundant(tenant, deps, &index, &idea).await? {
            deps.ideas.set_ingest_state(tenant, idea.id, "skipped_redundant").await?;
            report.skipped_redundant = true;
            return Ok(report);
        }
        // Stage 3 — single extraction (conversation mode; S2-R8). The capture body is fenced into the
        // tail by the analyzer's user message (the analyzer marks is_untrusted); here we fence it so the
        // reserved-tag namespace can never reach a prompt unescaped (S2-R51).
        deps.cost.check_before_call(tenant, user).await?;
        let analyzer = SourceAnalyzer::new(deps.llm, &deps.language);
        let fenced_body = fence_untrusted(&idea.origin, &idea.body);
        let (sa, _calls) =
            analyzer.analyze(tenant, &fenced_body, AnalyzeMode::Conversation).await?;
        // Stage 4 — source summary page with a SEMANTIC slug (S2-R8); tags INHERITED, not LLM-derived.
        deps.cost.check_before_call(tenant, user).await?;
        let summary_body = deps
            .llm
            .create_message(
                tenant,
                WikiReq {
                    system: SystemBlocks {
                        stable_prefix: crate::prompts::constraints::build_page_generation_prompt(&[]),
                    },
                    messages: vec![user_msg(&format!(
                        "Write a source summary for:\n{fenced_body}"
                    ))],
                    response_format: None,
                    max_tokens: 2048,
                    cache_breakpoint: Some(1),
                    disable_thinking: true,
                },
            )
            .await?
            .content;
        // Title/summary come from extraction; when a degenerate extraction left them blank, derive a
        // fallback from the capture body so each note gets a DISTINCT, meaningful slug — not a shared
        // 'untitled' page the next note would clobber. The fallback slug carries a short idea-id suffix
        // so even two notes with the same opening line never collide (S2-R8).
        // DIG-R4 — if this idea already produced a SOURCE page (edited-idea re-ingest), reuse that page's
        // slug so the write-gate upsert updates it IN PLACE rather than minting a new slug and orphaning
        // the old one. None ⇒ first ingest, derive a fresh slug below.
        let prior_slug = deps.ideas.source_page_slug(tenant, idea.id).await?;
        let (src_title, src_slug, src_summary) = if sa.source_title.trim().is_empty() {
            let t = fallback_title(&idea.body);
            let slug = prior_slug
                .clone()
                .unwrap_or_else(|| format!("{}-{}", slugify(&t), short_idea_id(idea.id)));
            let summary = if sa.summary.trim().is_empty() {
                fallback_summary(&idea.body)
            } else {
                sa.summary.clone()
            };
            (t, slug, summary)
        } else {
            let slug = prior_slug.unwrap_or_else(|| slugify(&sa.source_title));
            (sa.source_title.clone(), slug, sa.summary.clone())
        };
        let gate = WikiWriteGate::new(
            WikiRepo::new(deps.repo.pool()),
            crate::sandbox::TenantSandbox {
                vault_root: deps.vault_root.clone(),
                quota: crate::sandbox::TenantQuota::from_env(),
            },
        );
        let src_id = gate
            .write_page(
                tenant,
                PageWrite {
                    r#type: "source".into(),
                    slug: src_slug,
                    title: src_title,
                    aliases: vec![],
                    tags: deps.source_tags.clone(), // S2-R3 — inherited, NEVER LLM-derived
                    summary: src_summary,
                    source_ids: vec![idea.id],
                    body: summary_body,
                    llm_created: None,
                    llm_reviewed: None,
                },
            )
            .await?;
        report.created_pages.push(src_id);

        // Stage 5 — entity/concept CRUD with bounded concurrency + per-item isolation (S2-R10). Each
        // item: cost-cap → LLM body call → create_or_update through the write-gate. A failed item is
        // recorded in report.errors and does NOT abort the batch.
        let items: Vec<(String, String)> = sa
            .entities
            .iter()
            .map(|e| (e.name.clone(), "entity".to_string()))
            .chain(sa.concepts.iter().map(|c| (c.name.clone(), "concept".to_string())))
            .collect();
        let sem = std::sync::Arc::new(Semaphore::new(deps.concurrency));
        let mut futs = FuturesUnordered::new();
        for (name, ty) in items {
            if cancel.is_cancelled() {
                break;
            }
            // cost-cap BEFORE queueing work for this item (S2-R19/R64) — aborts cleanly if exhausted.
            deps.cost.check_before_call(tenant, user).await?;
            let pool = deps.repo.pool();
            let root = deps.vault_root.clone();
            let llm = deps.llm;
            let idea_id = idea.id;
            let sem = sem.clone();
            futs.push(async move {
                // Acquire the permit INSIDE the future (driven by FuturesUnordered), so the producer
                // loop never blocks on it — that would deadlock when concurrency < item count.
                let _permit = sem
                    .acquire_owned()
                    .await
                    .map_err(|_| "ingest semaphore closed".to_string())?;
                let g = WikiWriteGate::new(
                    WikiRepo::new(pool.clone()),
                    crate::sandbox::TenantSandbox {
                        vault_root: root,
                        quota: crate::sandbox::TenantQuota::from_env(),
                    },
                );
                let factory = PageFactory::new(llm, g, WikiRepo::new(pool));
                let body = llm
                    .create_message(
                        tenant,
                        WikiReq {
                            system: SystemBlocks {
                                stable_prefix:
                                    crate::prompts::constraints::build_page_generation_prompt(&[]),
                            },
                            messages: vec![user_msg(&format!("Write a {ty} page for {name}."))],
                            response_format: None,
                            max_tokens: 1024,
                            cache_breakpoint: Some(1),
                            disable_thinking: true,
                        },
                    )
                    .await
                    .map_err(|e| e.to_string())?
                    .content;
                let out = factory
                    .create_or_update(
                        tenant,
                        CreateOrUpdate {
                            name,
                            r#type: ty,
                            proposed_body: body,
                            aliases: vec![],
                            tags: vec![],
                            summary: String::new(),
                            source_id: Some(idea_id),
                        },
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<(Uuid, bool), String>((out.page_id, out.created))
            });
        }
        while let Some(joined) = futs.next().await {
            match joined {
                Ok((id, created)) => {
                    if created {
                        report.created_pages.push(id)
                    } else {
                        report.merged_pages.push(id)
                    }
                }
                Err(e) => report.errors.push(e),
            }
        }

        // Stage 6 — related-pages weave folds into create_or_update merges; regen the index (S2-R11);
        // transition the idea to 'ingested'. Re-running this job is idempotent (the slug-unique upserts
        // and ON CONFLICT merges make a second run a no-op write).
        let _ = regenerate_index(tenant, &deps.repo).await?;
        deps.ideas.set_ingest_state(tenant, idea.id, "ingested").await?;
        deps.ideas.set_last_ingested(tenant, idea.id).await?; // DIG-R1 — mark this capture as freshly distilled
        Ok(report)
    }

    async fn is_redundant<L: WikiLlm + ?Sized>(
        tenant: Uuid,
        deps: &IngestDeps<'_, L>,
        index: &str,
        idea: &IdeaInput,
    ) -> anyhow::Result<bool> {
        let fenced = fence_untrusted(&idea.origin, &idea.body);
        let resp = deps
            .llm
            .create_message(
                tenant,
                WikiReq {
                    system: SystemBlocks {
                        stable_prefix: format!(
                            "Decide if the NEW capture is fully_redundant vs the INDEX. Reply JSON {{\"fully_redundant\":bool}}.\nINDEX:\n{index}"
                        ),
                    },
                    messages: vec![user_msg(&fenced)],
                    response_format: None,
                    max_tokens: 64,
                    cache_breakpoint: Some(1),
                    disable_thinking: true,
                },
            )
            .await?;
        let v = parse_json_response(&resp.content)
            .unwrap_or(serde_json::json!({"fully_redundant": false}));
        Ok(v.get("fully_redundant").and_then(|x| x.as_bool()).unwrap_or(false))
    }
}

fn user_msg(s: &str) -> Message {
    Message {
        role: Role::User,
        content: Some(s.to_string()),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        finish_reason: None,
        reasoning: None,
        provider_data: None,
        active: true,
        is_untrusted: true,
    }
}

/// A human title derived from the capture body when extraction produced none: the first non-empty line
/// (or its first sentence), trimmed to a slug-friendly length. Never empty.
fn fallback_title(body: &str) -> String {
    let line = body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let sentence = line.split(['.', '。', '!', '?', '\n']).next().unwrap_or(line).trim();
    let title: String = sentence.chars().take(80).collect();
    let title = title.trim();
    if title.is_empty() { "Untitled note".to_string() } else { title.to_string() }
}

/// A short summary derived from the capture body when extraction produced none: leading prose,
/// whitespace-normalized and capped.
fn fallback_summary(body: &str) -> String {
    body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect()
}

/// First 8 hex chars of the idea uuid — a stable, collision-free slug suffix for fallback titles.
fn short_idea_id(id: Uuid) -> String {
    id.simple().to_string().chars().take(8).collect()
}

#[cfg(test)]
mod fallback_tests {
    use super::*;

    #[test]
    fn fallback_title_takes_the_first_sentence() {
        assert_eq!(
            fallback_title("Zephyr backend uses PostgreSQL 16 and Redis 7. More detail here."),
            "Zephyr backend uses PostgreSQL 16 and Redis 7"
        );
    }

    #[test]
    fn fallback_title_never_empty() {
        assert_eq!(fallback_title("   \n  "), "Untitled note");
    }

    #[test]
    fn short_idea_id_is_eight_hex() {
        let id = Uuid::nil();
        assert_eq!(short_idea_id(id), "00000000");
    }
}
