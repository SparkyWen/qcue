// QCue S2-R8/R13/R14/R15/R17/R18 — extraction loop: single call (conversation) / iterative batch (long source).
//
// The analyzer is the only place the extract helpers are sequenced into the WikiLlm seam. Conversation
// mode is ONE call (S2-R8 — captures are short and self-contained); long sources loop with the
// already-extracted exclusion list + merge/dedup + convergence detector. Output language is threaded
// into the (cache-safe) system prefix (S2-R18); names are never translated.
use crate::extract::{
    batch_limits::{calculate_batch_limits, Granularity},
    batch_merger::{merge_batch_results, ExtractedItem},
    convergence::{detect_convergence, Convergence, RoundState},
    normalize::{normalize_batch_response, Validity},
};
use crate::json_hardening::{next_max_tokens_on_truncation, parse_json_response, PREFILL_OPEN_BRACE};
use crate::llm::{SystemBlocks, WikiLlm, WikiLlmError, WikiReq};
use crate::prompts::constraints::UNIVERSAL_LINK_CONSTRAINTS;
use crate::types::SourceAnalysis;
use protocol::{Message, Role};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzeMode {
    Conversation,
    LongSource,
}

pub struct SourceAnalyzer<'a, L: WikiLlm + ?Sized> {
    llm: &'a L,
    language: String,
}

impl<'a, L: WikiLlm + ?Sized> SourceAnalyzer<'a, L> {
    pub fn new(llm: &'a L, language: &str) -> Self {
        Self { llm, language: language.to_string() }
    }

    fn system(&self) -> SystemBlocks {
        SystemBlocks {
            stable_prefix: format!(
                "You extract a source's title, a one-paragraph summary, and its entities and concepts. \
                 Output language: {}. Names are NOT translated.\n{}",
                self.language, UNIVERSAL_LINK_CONSTRAINTS
            ),
        }
    }

    fn user(&self, content: &str, exclude: &[String]) -> Vec<Message> {
        let excl = if exclude.is_empty() {
            String::new()
        } else {
            format!("\nAlready extracted (do not repeat): {}", exclude.join(", "))
        };
        vec![user_msg(&format!(
            "{content}{excl}\nReturn ONLY JSON with keys: source_title (a short, specific title for this \
             source — never empty), summary (one paragraph), entities, concepts, contradictions, \
             related_pages, key_points."
        ))]
    }

    /// Returns (analysis, llm_call_count). Conversation = exactly one call (S2-R8).
    pub async fn analyze(
        &self,
        tenant: Uuid,
        content: &str,
        mode: AnalyzeMode,
    ) -> Result<(SourceAnalysis, u32), WikiLlmError> {
        match mode {
            AnalyzeMode::Conversation => {
                let sa = self.one_call(tenant, content, &[], &mut 1024).await?;
                Ok((sa, 1))
            }
            AnalyzeMode::LongSource => self.batch_loop(tenant, content).await,
        }
    }

    async fn one_call(
        &self,
        tenant: Uuid,
        content: &str,
        exclude: &[String],
        max_tokens: &mut u32,
    ) -> Result<SourceAnalysis, WikiLlmError> {
        let req = WikiReq {
            system: self.system(),
            messages: self.user(content, exclude),
            response_format: None,
            max_tokens: *max_tokens,
            cache_breakpoint: Some(1),
            disable_thinking: true,
        };
        let mut resp = self.llm.create_message(tenant, req).await?;
        if resp.truncated {
            // S2-R14: double max_tokens once on truncation and retry.
            if let Some(doubled) = next_max_tokens_on_truncation(*max_tokens) {
                *max_tokens = doubled;
                let req = WikiReq {
                    system: self.system(),
                    messages: self.user(content, exclude),
                    response_format: None,
                    max_tokens: doubled,
                    cache_breakpoint: Some(1),
                    disable_thinking: true,
                };
                resp = self.llm.create_message(tenant, req).await?;
            }
        }
        // prefill `{` robustness: retry parse with a restored leading brace, then plain.
        let prefilled = format!("{PREFILL_OPEN_BRACE}{}", resp.content.trim_start_matches('{'));
        let v = parse_json_response(&prefilled)
            .or_else(|_| parse_json_response(&resp.content))
            .map_err(|e| WikiLlmError::Provider(e.to_string()))?;
        Ok(coerce_source_analysis(v))
    }

    async fn batch_loop(
        &self,
        tenant: Uuid,
        content: &str,
    ) -> Result<(SourceAnalysis, u32), WikiLlmError> {
        let mut limits = calculate_batch_limits(content.len(), Granularity::Standard);
        let mut acc_e: Vec<ExtractedItem> = vec![];
        let mut acc_c: Vec<ExtractedItem> = vec![];
        let mut calls = 0u32;
        let mut halved = false;
        let mut first: Option<SourceAnalysis> = None;
        loop {
            let exclude: Vec<String> =
                acc_e.iter().chain(acc_c.iter()).map(|i| i.name.clone()).collect();
            let mut mt = limits.max_tokens;
            let sa = self.one_call(tenant, content, &exclude, &mut mt).await?;
            calls += 1;
            if first.is_none() {
                first = Some(sa.clone());
            }
            let raw = serde_json::to_value(&sa).unwrap_or_default();
            let norm = normalize_batch_response(&raw);
            let before = acc_e.len() + acc_c.len();
            acc_e = merge_batch_results(std::mem::take(&mut acc_e), norm.entities);
            acc_c = merge_batch_results(std::mem::take(&mut acc_c), norm.concepts);
            let new_count = (acc_e.len() + acc_c.len()).saturating_sub(before);
            let state = RoundState {
                batch_size: limits.items_per_batch,
                new_this_round: new_count,
                already_halved: halved,
                cumulative_items: acc_e.len() + acc_c.len(),
                cap: limits.max_total_items,
                empty_or_all_dup: norm.validity != Validity::Valid,
            };
            match detect_convergence(&state) {
                Convergence::Continue => {}
                Convergence::Halve => {
                    limits.items_per_batch = (limits.items_per_batch / 2).max(1);
                    halved = true;
                }
                Convergence::Stop => break,
            }
        }
        let mut sa = first.unwrap_or_default();
        sa.entities = acc_e
            .into_iter()
            .map(|i| crate::types::ItemInfo { name: i.name, aliases: i.aliases, subtype: None })
            .collect();
        sa.concepts = acc_c
            .into_iter()
            .map(|i| crate::types::ItemInfo { name: i.name, aliases: i.aliases, subtype: None })
            .collect();
        Ok((sa, calls))
    }
}

pub(crate) fn user_msg(s: &str) -> Message {
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

/// Strict parse first (honors `SourceAnalysis`'s `deny_unknown_fields` when the model is clean), else
/// pull KNOWN fields individually so a model that adds ONE extra key doesn't nuke the whole extraction
/// to empty — the silent-blank root cause (`from_value(...).unwrap_or_default()`). Per-item extras are
/// dropped, not fatal.
fn coerce_source_analysis(v: serde_json::Value) -> SourceAnalysis {
    if let Ok(sa) = serde_json::from_value::<SourceAnalysis>(v.clone()) {
        return sa;
    }
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let items = |k: &str| -> Vec<crate::types::ItemInfo> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|it| serde_json::from_value(it.clone()).ok()).collect())
            .unwrap_or_default()
    };
    let list = |k: &str| -> Vec<String> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    SourceAnalysis {
        source_title: s("source_title"),
        summary: s("summary"),
        entities: items("entities"),
        concepts: items("concepts"),
        contradictions: v
            .get("contradictions")
            .cloned()
            .and_then(|x| serde_json::from_value(x).ok())
            .unwrap_or_default(),
        related_pages: list("related_pages"),
        key_points: list("key_points"),
    }
}

#[cfg(test)]
mod coerce_tests {
    use super::coerce_source_analysis;
    use serde_json::json;

    #[test]
    fn keeps_known_fields_despite_an_unknown_key() {
        // strict from_value would error on `surprise` (deny_unknown_fields) and the old code nuked the
        // whole extraction to default() — losing the title/entities. Coercion keeps them.
        let v = json!({
            "source_title": "Zephyr backend", "summary": "stack notes",
            "entities": [{"name":"PostgreSQL","aliases":[]}], "concepts": [],
            "contradictions": [], "related_pages": [], "key_points": [],
            "surprise": true
        });
        let sa = coerce_source_analysis(v);
        assert_eq!(sa.source_title, "Zephyr backend");
        assert_eq!(sa.summary, "stack notes");
        assert_eq!(sa.entities.len(), 1);
    }

    #[test]
    fn clean_json_still_parses_strictly() {
        let v = json!({
            "source_title": "T", "summary": "S", "entities": [], "concepts": [],
            "contradictions": [], "related_pages": [], "key_points": []
        });
        assert_eq!(coerce_source_analysis(v).source_title, "T");
    }
}
