// QCue S2-R57/R59 / A-R12..R16,R19 — the harness-driven READ-ONLY forked Dream agent. It drives the
// LLM through the `wiki::llm::WikiLlm` seam with the dream tool policy (`build_tool_policy(true)`: the
// read-only recall_search/read_page/read_lines surface PLUS propose_* — "one sandbox, two prompts").
//
// Faithful fidelity points (App. A §2.4-2.6):
//   - A-R14 cache-safe fork: a STABLE byte-fixed system prefix; the 4-phase prompt is appended to the
//     TAIL (no volatile bytes in the prefix → the fork rides the warm prefix).
//   - A-R13 the MODEL generates its own narrow grep patterns over the JSONL via recall_search — the
//     harness never rewrites them (this IS recall applied to consolidation).
//   - A-R20 cost-cap checked BEFORE every provider call; a $0 ledger aborts before any call so the
//     scheduler rolls the clock back (the scan-throttle is the backoff).
//   - A-R16 skip_transcript: the dream is ephemeral; it does NOT write the tenant's main `messages`.
//   - A-R19 writes are PROPOSED: merges/deletes route through the candidates→confirm gate (the
//     `approvals` table) + reversible soft-delete; low-risk edits would auto-apply.
//   - A-R12 propose_write targets are realpath-guarded to the tenant wiki root (the isAutoMemPath
//     analog); network egress is off (the tool policy carries `network_off`).
//   - A-R15 the progress watcher collects propose_* target paths into `files_touched` (dedup), feeding
//     the "Improved N pages" report.
use crate::recall::tool_policy::{build_tool_policy, ToolPolicy};
use chrono::{DateTime, Utc};
use protocol::{Message, Role};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wiki::approvals::{route_destructive, DestructiveOp};
use wiki::cost::CostGuard;
use wiki::dream::scheduler::{DreamConfig, DreamOutcome, DreamRunner};
use wiki::llm::{SystemBlocks, WikiLlm, WikiReq};
use wiki::path_guard::resolve_in_root;
use wiki::prompts::consolidation::build_consolidation_prompt;

/// The harness-driven read-only fork. Holds the LLM seam, the per-tenant vault root (for propose-path
/// realpath-guarding), and the cost guard (cost-cap-before-call + the approvals-gate pool).
pub struct DreamAgent<'a, L: WikiLlm + ?Sized> {
    llm: &'a L,
    vault_root: std::path::PathBuf,
    cost: &'a CostGuard,
    cfg: DreamConfig,
}

impl<'a, L: WikiLlm + ?Sized> DreamAgent<'a, L> {
    pub fn new(llm: &'a L, vault_root: std::path::PathBuf, cost: &'a CostGuard) -> Self {
        Self { llm, vault_root, cost, cfg: DreamConfig::default() }
    }

    /// A-R40 — the SAME builder recall uses; Dream adds propose_* (`allow_propose=true`). The read-only
    /// / network-off / root-confined invariants are identical (the lone difference is the propose tools).
    pub fn tool_policy(&self) -> ToolPolicy {
        build_tool_policy(true, false) // Dream stays OFFLINE (no web): deterministic consolidation.
    }

    /// The read-only fork loop: cost-cap-before-call (A-R20) → 4-phase prompt in the TAIL behind a
    /// stable prefix (A-R14) → drive the LLM (the model emits recall_search / propose_* — A-R13/R19) →
    /// collect files_touched (A-R15). Does NOT write the main transcript (A-R16).
    pub async fn run(
        &self,
        tenant: Uuid,
        user: Uuid,
        since: DateTime<Utc>,
        cancel: CancellationToken,
    ) -> anyhow::Result<DreamOutcome> {
        if cancel.is_cancelled() {
            anyhow::bail!("dream cancelled before start");
        }
        // A-R20 / S2-R60 — cost cap checked BEFORE the provider call. A $0 ledger aborts here (zero
        // provider calls) so the scheduler rewinds the clock.
        self.cost.check_before_call(tenant, user).await?;

        let extra = format!(
            "Tools are READ-ONLY except propose_edit/propose_write (which route through confirm). \
             Sessions since: {since}"
        );
        let prompt = build_consolidation_prompt(&extra);
        let req = WikiReq {
            // A-R14 — a stable, byte-fixed prefix (no timestamps/counters); the fork rides the warm cache.
            system: SystemBlocks {
                stable_prefix: "You are the QCue Dream consolidation agent (read-only fork).".into(),
            },
            // The 4-phase prompt is appended to the TAIL (RKM §7 #3).
            messages: vec![Message {
                role: Role::User,
                content: Some(prompt),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
                finish_reason: None,
                reasoning: None,
                provider_data: None,
                active: true,
                is_untrusted: false,
            }],
            response_format: None,
            max_tokens: 4096,
            cache_breakpoint: Some(1),
            disable_thinking: false,
        };
        // A-R16 — this turn loop does NOT append to the main `messages` transcript (the dream is
        // ephemeral; its provenance lives in jobs + the dream log).
        let resp = self.llm.create_message(tenant, req).await?;
        // A-R15 — collect propose_* target paths into files_touched (dedup, root-confined). The router
        // turn loop dispatches the model's tool calls INTERNALLY and surfaces only the final content
        // (`WikiResp.content`), so we extract the proposed targets the model named in that content
        // (propose_write/propose_edit JSON args + [[wiki-link]] slugs), then validate each under the
        // tenant vault root. A scripted StubProvider returns plain text → no targets (files_touched
        // stays empty, as before); a real model that names proposed pages fills the feed.
        let files_touched = collect_files_touched(&self.vault_root, &resp.content);
        Ok(DreamOutcome { files_touched, turns: 1 })
    }

    /// S2-R59 / A-R19 — a proposed page MERGE routes through the SAME candidates→confirm gate the lint
    /// dup-merge uses (one gate, one table). It lands as a PENDING `approvals` row (action `wiki_merge`)
    /// plus a reversible soft-delete of the merge source; canonical is unchanged until a confirm
    /// endpoint promotes it.
    pub async fn propose_merge(
        &self,
        tenant: Uuid,
        user: Uuid,
        from: Uuid,
        into: Uuid,
    ) -> anyhow::Result<()> {
        let pool = self.cost.pool();
        route_destructive(&pool, tenant, user, "dream", DestructiveOp::WikiMerge { from, into }).await?;
        Ok(())
    }

    /// A-R12 — a proposed WRITE is realpath-guarded to the tenant root (the isAutoMemPath analog) before
    /// any candidate is enqueued. Traversal / out-of-root / non-.md / over-cap are rejected. (The live
    /// candidate enqueue lands with S1's tool dispatch; here the guard is the load-bearing invariant.)
    pub async fn propose_write(
        &self,
        _tenant: Uuid,
        _user: Uuid,
        rel: &str,
        _body: &str,
    ) -> anyhow::Result<()> {
        resolve_in_root(&self.vault_root, rel, 1_000_000)?;
        Ok(())
    }
}

// The agent IS the scheduler's `DreamRunner` seam — `wiki::DreamScheduler` drives it without reaching
// the router itself (keeping `wiki` provider-agnostic).
#[async_trait::async_trait]
impl<L: WikiLlm + ?Sized> DreamRunner for DreamAgent<'_, L> {
    async fn run(
        &self,
        tenant: Uuid,
        user: Uuid,
        since: DateTime<Utc>,
        cancel: CancellationToken,
    ) -> anyhow::Result<DreamOutcome> {
        DreamAgent::run(self, tenant, user, since, cancel).await
    }
}

impl<L: WikiLlm + ?Sized> DreamAgent<'_, L> {
    /// The configured caps (time-gate / min-sessions / scan-throttle) the live dispatch loop honors.
    pub fn config(&self) -> &DreamConfig {
        &self.cfg
    }
}

/// A-R15 — extract proposed target paths the model named in its final response and keep only those that
/// resolve UNDER `vault_root` (the realpath guard rejects traversal / out-of-root / non-.md). Deduped,
/// order-preserving. Inputs scanned: `propose_write`/`propose_edit` tool-call JSON args (`rel`/`path`/
/// `slug` keys) and `[[slug]]` wiki-links.
///
/// The router turn loop dispatches the model's tool calls INTERNALLY and surfaces only the final content
/// (`WikiResp.content`), so this scans that content for the proposed targets. A scripted StubProvider
/// returns plain text → no targets (files_touched stays empty); a real model that names proposed pages
/// fills the "Improved N pages" feed.
pub fn collect_files_touched(vault_root: &std::path::Path, content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push = |raw: &str, out: &mut Vec<String>| {
        let cand = raw.trim();
        if cand.is_empty() {
            return;
        }
        // Slugs without an extension are normalized to `<slug>.md` (the wiki page convention).
        let rel = if cand.ends_with(".md") { cand.to_string() } else { format!("{cand}.md") };
        // Root-confine: only keep targets the path guard accepts (traversal/out-of-root rejected).
        if resolve_in_root(vault_root, &rel, 1_000_000).is_ok() && !out.contains(&rel) {
            out.push(rel);
        }
    };
    // 1) propose_* JSON args: scan for `"rel"|"path"|"slug": "<value>"` occurrences.
    for key in ["\"rel\"", "\"path\"", "\"slug\""] {
        let mut rest = content;
        while let Some(i) = rest.find(key) {
            rest = &rest[i + key.len()..];
            // skip to the opening quote of the value, then capture up to the closing quote.
            if let Some(open) = rest.find('"') {
                let after = &rest[open + 1..];
                if let Some(close) = after.find('"') {
                    push(&after[..close], &mut out);
                    rest = &after[close + 1..];
                    continue;
                }
            }
            break;
        }
    }
    // 2) [[wiki-link]] slugs the model names as touched pages.
    let mut rest = content;
    while let Some(i) = rest.find("[[") {
        rest = &rest[i + 2..];
        if let Some(j) = rest.find("]]") {
            push(&rest[..j], &mut out);
            rest = &rest[j + 2..];
        } else {
            break;
        }
    }
    out
}

#[cfg(test)]
mod files_touched_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::collect_files_touched;

    #[test]
    fn collects_propose_args_and_wiki_links_root_confined_deduped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // a model "final answer" naming proposed pages two ways + a traversal attempt that must be dropped.
        let content = r#"
            I propose_write {"rel": "alpha.md", "body": "..."} and
            propose_edit {"slug": "beta"}. Also touched [[gamma]] and [[alpha]] (dup).
            Rejected: {"path": "../escape.md"} (out of root).
        "#;
        let got = collect_files_touched(root, content);
        // alpha (from rel + [[alpha]] dedup), beta (slug→.md), gamma ([[gamma]]→.md). escape dropped.
        assert!(got.contains(&"alpha.md".to_string()), "got: {got:?}");
        assert!(got.contains(&"beta.md".to_string()), "got: {got:?}");
        assert!(got.contains(&"gamma.md".to_string()), "got: {got:?}");
        assert!(!got.iter().any(|p| p.contains("escape")), "traversal must be rejected: {got:?}");
        // dedup: alpha appears once.
        assert_eq!(got.iter().filter(|p| *p == "alpha.md").count(), 1, "got: {got:?}");
    }

    #[test]
    fn plain_text_yields_empty() {
        let dir = tempfile::tempdir().unwrap();
        let got = collect_files_touched(dir.path(), "Just a plain summary with no proposals.");
        assert!(got.is_empty(), "got: {got:?}");
    }
}
