// QCue S2-R20/R22/R23/R24 — create vs merge vs collide; programmatic frontmatter + LLM body w/
// NO_NEW_CONTENT; reviewed append-only. The deterministic ConflictResolver gates the cheap path; the
// LLM body-merge runs only on a real merge (and returns NO_NEW_CONTENT to skip a no-op write). All body
// writes go through the single WikiWriteGate (pitfall #11).
use crate::conflict::{ConflictResolver, ExistingPage};
use crate::llm::{SystemBlocks, WikiLlm, WikiReq};
use crate::prompts::constraints::build_merge_prompt;
use crate::types::{ResolveAction, NO_NEW_CONTENT};
use crate::write_gate::{PageWrite, WikiWriteGate};
use protocol::{Message, Role};
use store::wiki_repo::WikiRepo;
use uuid::Uuid;

pub struct CreateOrUpdate {
    pub name: String,
    pub r#type: String,
    pub proposed_body: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub summary: String,
    pub source_id: Option<Uuid>,
}

pub struct PageOutcome {
    pub page_id: Uuid,
    pub created: bool,
    pub merged: bool,
    pub skipped_no_new_content: bool,
    pub cross_type_collision: bool,
}

pub struct PageFactory<'a, L: WikiLlm + ?Sized> {
    llm: &'a L,
    gate: WikiWriteGate,
    repo: WikiRepo,
}

impl<'a, L: WikiLlm + ?Sized> PageFactory<'a, L> {
    pub fn new(llm: &'a L, gate: WikiWriteGate, repo: WikiRepo) -> Self {
        Self { llm, gate, repo }
    }

    pub async fn create_or_update(
        &self,
        tenant: Uuid,
        c: CreateOrUpdate,
    ) -> anyhow::Result<PageOutcome> {
        let existing: Vec<ExistingPage> = self
            .repo
            .existing_pages(tenant)
            .await?
            .into_iter()
            .map(|p| ExistingPage {
                id: p.id,
                slug: p.slug,
                title: p.title,
                aliases: p.aliases,
                r#type: p.r#type,
            })
            .collect();
        let res = ConflictResolver::resolve(&c.name, &c.r#type, &existing);
        match res.action {
            ResolveAction::Create => {
                let id = self.write(tenant, &c, &c.proposed_body).await?;
                Ok(PageOutcome {
                    page_id: id,
                    created: true,
                    merged: false,
                    skipped_no_new_content: false,
                    cross_type_collision: false,
                })
            }
            ResolveAction::Merge | ResolveAction::Flag => {
                let target = res
                    .target
                    .ok_or_else(|| anyhow::anyhow!("merge resolution missing target"))?;
                let page = self.repo.page(tenant, target).await?;
                let cross = res.action == ResolveAction::Flag;
                if page.reviewed {
                    // S2-R23 — reviewed pages are append-only under a "New Information" section.
                    let body = self.gate.read_body(tenant, target).await?;
                    let merged = format!("{body}\n\n## New Information\n{}", c.proposed_body);
                    self.write_at(tenant, &page.r#type, &page.slug, &page.title, &merged, &c).await?;
                    return Ok(PageOutcome {
                        page_id: target,
                        created: false,
                        merged: true,
                        skipped_no_new_content: false,
                        cross_type_collision: cross,
                    });
                }
                // S2-R22 — LLM body merge; NO_NEW_CONTENT sentinel skips the write.
                let existing_body = self.gate.read_body(tenant, target).await?;
                let merged_body = self.llm_merge(tenant, &existing_body, &c.proposed_body).await?;
                if merged_body.trim() == NO_NEW_CONTENT {
                    return Ok(PageOutcome {
                        page_id: target,
                        created: false,
                        merged: false,
                        skipped_no_new_content: true,
                        cross_type_collision: cross,
                    });
                }
                self.write_at(tenant, &page.r#type, &page.slug, &page.title, &merged_body, &c).await?;
                Ok(PageOutcome {
                    page_id: target,
                    created: false,
                    merged: true,
                    skipped_no_new_content: false,
                    cross_type_collision: cross,
                })
            }
        }
    }

    async fn llm_merge(&self, tenant: Uuid, existing: &str, proposed: &str) -> anyhow::Result<String> {
        let req = WikiReq {
            system: SystemBlocks { stable_prefix: build_merge_prompt() },
            messages: vec![user_msg(&format!("EXISTING:\n{existing}\n\nNEW:\n{proposed}"))],
            response_format: None,
            max_tokens: 2048,
            cache_breakpoint: Some(1),
            disable_thinking: true,
        };
        Ok(self.llm.create_message(tenant, req).await?.content)
    }

    async fn write(&self, tenant: Uuid, c: &CreateOrUpdate, body: &str) -> anyhow::Result<Uuid> {
        self.gate
            .write_page(
                tenant,
                PageWrite {
                    r#type: c.r#type.clone(),
                    slug: slugify::slugify(&c.name),
                    title: c.name.clone(),
                    aliases: c.aliases.clone(),
                    tags: c.tags.clone(),
                    summary: c.summary.clone(),
                    source_ids: c.source_id.into_iter().collect(),
                    body: body.to_string(),
                    llm_created: None,
                    llm_reviewed: None,
                },
            )
            .await
    }

    async fn write_at(
        &self,
        tenant: Uuid,
        ty: &str,
        slug: &str,
        title: &str,
        body: &str,
        c: &CreateOrUpdate,
    ) -> anyhow::Result<Uuid> {
        self.gate
            .write_page(
                tenant,
                PageWrite {
                    r#type: ty.into(),
                    slug: slug.into(),
                    title: title.into(),
                    aliases: c.aliases.clone(),
                    tags: c.tags.clone(),
                    summary: c.summary.clone(),
                    source_ids: c.source_id.into_iter().collect(),
                    body: body.to_string(),
                    llm_created: None,
                    llm_reviewed: None,
                },
            )
            .await
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
