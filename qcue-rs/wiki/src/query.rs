// QCue S2-R26..R30 — index-first retrieval/synthesis: read the index FIRST, tiered page selection
// (L1 local keyword / L2 narrow-then-refine / L3 full scan), load selected bodies truncated, then
// synthesize under the STRICT rules (answer from the WIKI not general knowledge; `[[wikilinks]]` only;
// a mandatory `## References` citing every page) and parse the references into citations.
//
// NO embeddings (D14, pitfall #13) — the index is the retrieval substrate. The pgvector upgrade is a
// noted M6+ trigger (manifest > a few hundred pages OR recall p95 regression), NOT built here.
//
// `recall_query(tenant, question, sink)` is left as a clean seam for the S3 recall-SSE handler: the
// streaming variant will run a recall turn via `router::run_turn` with `recall_search`/`read_page`/
// `read_lines` available so the MODEL searches, then synthesize. This milestone ships the non-streaming
// `QueryEngine::answer` (index-first synthesis + file-the-answer-back); the SSE wire is the S3-finish
// milestone.
use crate::index_gen::regenerate_index;
use crate::llm::{SystemBlocks, WikiLlm, WikiReq};
use crate::prompts::constraints::build_synthesis_prompt;
use fence::fence_untrusted;
use protocol::{Citation, Message, Role};
use std::path::PathBuf;
use store::wiki_repo::WikiRepo;
use uuid::Uuid;

pub const MAX_PAGE_CONTENT_CHARS: usize = 12_800; // constants.ts:152
const L1_MIN_PAGES: usize = 3;
const L1_MIN_SCORE: usize = 6;

/// A catalog row projected from the PG mirror (never a body read, pitfall #12).
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub aliases: Vec<String>,
}

/// The tiered-selection outcome (S2-R27). L1 = strong local keyword hit (no LLM); L2 = a narrower set
/// to refine; L3 = full scan (the index is the only signal).
#[derive(Debug)]
pub enum Selection {
    Layer1(Vec<String>),
    Layer2(Vec<String>),
    Layer3,
}

/// S2-R27 — tiered selection: L1 local keyword (no LLM), L2 narrow-then-refine, L3 full scan.
pub fn select_pages(query: &str, catalog: &[CatalogEntry]) -> Selection {
    let terms: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
    if terms.is_empty() {
        return Selection::Layer3;
    }
    let mut scored: Vec<(String, usize)> = catalog
        .iter()
        .map(|c| {
            let hay = format!(
                "{} {} {}",
                c.title.to_lowercase(),
                c.summary.to_lowercase(),
                c.aliases.join(" ").to_lowercase()
            );
            let score = terms.iter().filter(|t| hay.contains(t.as_str())).count();
            (c.slug.clone(), score)
        })
        .collect();
    scored.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
    let strong: Vec<String> =
        scored.iter().filter(|(_, s)| *s >= L1_MIN_SCORE).map(|(sl, _)| sl.clone()).collect();
    if strong.len() >= L1_MIN_PAGES {
        return Selection::Layer1(strong.into_iter().take(5).collect());
    }
    let any: Vec<String> =
        scored.iter().filter(|(_, s)| *s > 0).map(|(sl, _)| sl.clone()).take(15).collect();
    if any.is_empty() {
        Selection::Layer3
    } else {
        Selection::Layer2(any)
    }
}

/// A synthesized answer + the citations parsed from its `## References` block.
pub struct Answer {
    pub text: String,
    pub citations: Vec<Citation>,
}

/// The index-first query engine. Provider-agnostic: the LLM is reached only through `WikiLlm`.
pub struct QueryEngine<'a, L: WikiLlm> {
    llm: &'a L,
    repo: WikiRepo,
    vault_root: PathBuf,
}

impl<'a, L: WikiLlm> QueryEngine<'a, L> {
    pub fn new(llm: &'a L, repo: WikiRepo, vault_root: PathBuf) -> Self {
        Self { llm, repo, vault_root }
    }

    /// S2-R26..R30 — read the index, select pages, load truncated bodies, synthesize under the strict
    /// rules, and parse the references. The user query is UNTRUSTED → it lives fenced in the message
    /// TAIL only (pitfall #1/#2), never the stable prefix.
    pub async fn answer(&self, tenant: Uuid, query: &str) -> anyhow::Result<Answer> {
        // S2-R26 — read the index FIRST. An empty wiki yields the "(wiki is empty)" sentinel so the
        // synthesis prompt can branch on it instead of inventing an answer.
        let index = regenerate_index(tenant, &self.repo).await?;
        let catalog: Vec<CatalogEntry> = self
            .repo
            .catalog_rows(tenant)
            .await?
            .into_iter()
            .map(|(slug, title, summary, aliases)| CatalogEntry { slug, title, summary, aliases })
            .collect();
        let chosen = match select_pages(query, &catalog) {
            Selection::Layer1(s) | Selection::Layer2(s) => s,
            Selection::Layer3 => catalog.iter().map(|c| c.slug.clone()).take(5).collect(),
        };
        // S2-R28 — load each selected body, truncated to MAX_PAGE_CONTENT_CHARS (the index already
        // bounds the manifest; this bounds each page's contribution).
        let mut loaded_titles = Vec::new();
        let mut bodies = String::new();
        for slug in &chosen {
            if let Ok(Some(body_ref)) = self.repo.body_ref_by_slug(tenant, slug).await
                // defense-in-depth: the stored body_ref must resolve UNDER this tenant's vault root
                // (the mirror is system-set, but never trust a path into the filesystem blindly).
                && self.under_vault_root(&body_ref)
                && let Ok(body) = tokio::fs::read_to_string(&body_ref).await
            {
                let t: String = body.chars().take(MAX_PAGE_CONTENT_CHARS).collect();
                bodies.push_str(&format!("\n# [[{slug}]]\n{t}\n"));
                loaded_titles.push(slug.clone());
            }
        }
        // S2-R29 — synthesis: wiki-only, [[links]] only, mandatory ## References. The strict rules live
        // in the stable prefix (build_synthesis_prompt); the untrusted query is fenced in the tail.
        let req = WikiReq {
            system: SystemBlocks { stable_prefix: build_synthesis_prompt(&index, &loaded_titles) },
            messages: vec![Message {
                role: Role::User,
                content: Some(format!("{}\n\nWIKI PAGES:\n{}", fence_untrusted("user_query", query), bodies)),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
                finish_reason: None,
                reasoning: None,
                provider_data: None,
                active: true,
                is_untrusted: true,
            }],
            response_format: None,
            max_tokens: 2048,
            cache_breakpoint: Some(1),
            disable_thinking: false,
        };
        let text = self.llm.create_message(tenant, req).await?.content;
        let citations = parse_references(&text);
        Ok(Answer { text, citations })
    }

    /// True iff `body_ref` (the stored absolute vault path) resolves under this tenant's vault root.
    /// A non-existent or out-of-root path returns false (skip the load; never panic, never escape).
    fn under_vault_root(&self, body_ref: &str) -> bool {
        match (std::fs::canonicalize(&self.vault_root), std::fs::canonicalize(body_ref)) {
            (Ok(root), Ok(path)) => path.starts_with(&root),
            _ => false,
        }
    }
}

/// The recall-answer sink the S3 SSE handler will drive. The S3-finish milestone implements a streaming
/// sink over the Appendix A §3.4 recall SSE taxonomy (`?token=` auth); this milestone defines the seam
/// so the handler signature is stable. `emit_answer` is called once with the synthesized answer.
#[async_trait::async_trait]
pub trait RecallSink: Send {
    async fn emit_answer(&mut self, answer: &Answer) -> anyhow::Result<()>;
}

/// THE seam the S3 recall-SSE endpoint calls: run an index-first recall/query for `question` and push
/// the synthesized answer to `sink`. NON-streaming today (one `emit_answer`); the streaming/agentic
/// variant (a recall turn via `router::run_turn` with `recall_search`/`read_page`/`read_lines` so the
/// MODEL searches, then synthesize) lands with the S3-finish milestone — the signature stays the same.
pub async fn recall_query<L: WikiLlm, S: RecallSink>(
    tenant: Uuid,
    question: &str,
    engine: &QueryEngine<'_, L>,
    sink: &mut S,
) -> anyhow::Result<()> {
    let answer = engine.answer(tenant, question).await?;
    sink.emit_answer(&answer).await
}

/// Parse the `## References` block into `Citation{rel_path,start_line,end_line}` (lines default 0 — the
/// synthesis cites whole pages by slug, not line ranges; A-R25 keeps the path conservative). Public so
/// the agentic recall stream can reuse the SAME citation parser on the model's authored answer.
pub fn parse_references(text: &str) -> Vec<Citation> {
    let mut out = Vec::new();
    if let Some(idx) = text.find("## References") {
        for line in text[idx..].lines().skip(1) {
            if let Some(start) = line.find("[[")
                && let Some(end) = line[start..].find("]]")
            {
                let inner = &line[start + 2..start + end];
                let slug = inner.split('|').next().unwrap_or(inner).trim();
                if !slug.is_empty() {
                    out.push(Citation { rel_path: format!("{slug}.md"), start_line: 0, end_line: 0 });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    fn cat(slug: &str, kw: &[&str]) -> CatalogEntry {
        CatalogEntry { slug: slug.into(), title: slug.into(), summary: kw.join(" "), aliases: vec![] }
    }
    #[test]
    fn tiered_selection_layers() {
        // DEVIATION (recorded): the plan's L1 example query ("rust async runtime memory safety
        // systems") can't reach L1_MIN_SCORE=6 against the plan's own catalog (no page contains all 6
        // distinct terms), so — as Task 4 did for the route_search prose vs code — the authoritative
        // `select_pages` function is followed: a query whose terms all land on one page DOES reach L1;
        // a no-local-match query falls to L3 (full scan). The load-bearing distinction (local selection
        // vs full scan) is asserted directly.
        let catalog = vec![
            // a page whose summary contains every one of the 6 query terms → score 6 → L1-eligible.
            cat("rust", &["rust", "async", "runtime", "memory", "safety", "systems"]),
            cat("rustlang", &["rust", "async", "runtime", "memory", "safety", "systems"]),
            cat("rustbook", &["rust", "async", "runtime", "memory", "safety", "systems"]),
            cat("graphs", &["graph", "node", "edge"]),
        ];
        // L1: ≥3 pages score ≥6 → Layer1, no LLM.
        assert!(matches!(
            select_pages("rust async runtime memory safety systems", &catalog),
            Selection::Layer1(_)
        ));
        // L2: a partial local match (some terms hit, < L1 threshold) → narrow set, no full scan.
        assert!(matches!(select_pages("graph node", &catalog), Selection::Layer2(_)));
        // L3: empty-keyword / no local match → Layer3 (full LLM scan).
        assert!(matches!(select_pages("zzzz", &catalog), Selection::Layer3));
    }
    #[test]
    fn parse_references_extracts_slugs() {
        let txt = "answer [[rust]]\n\n## References\n- [[rust|Rust]] — the entity page\n- [[tokio]] — runtime";
        let cites = parse_references(txt);
        assert_eq!(cites.len(), 2);
        assert_eq!(cites[0].rel_path, "rust.md");
        assert_eq!(cites[1].rel_path, "tokio.md");
    }
}
