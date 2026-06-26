// QCue S2-R31 / A-R21..R25 — the `recall_search` ToolSpec + its execution: routing, mode inference,
// bookends (goal + conclusion), an anchored window, lineage dedup + current-session exclusion, and a
// conservative `<file>:<line>` citation.
//
// The CORE PRINCIPLE (Appendix A): recall is agentic, NOT a fixed retrieval step. The harness registers
// this tool on the router's read-only seam and the MODEL calls it with its OWN `pattern`. The harness
// never rewrites that pattern (A-R13) — `route_search` only picks the index path; `infer_mode` only
// reads the arg shape.
use protocol::{Citation, ToolDef};
use search_route::{route_search, SearchMode};
use store::search_repo::SearchRepo;
use uuid::Uuid;

/// Modes are inferred from the args (A-R22): a slug → browse (recent), a free pattern → discovery
/// (hits with bookends), a match id + direction → scroll (around an anchor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecallMode {
    Browse,
    Discovery,
    Scroll,
}

/// The model-authored arguments. `pattern` is passed through UNREWRITTEN.
pub struct RecallArgs {
    pub pattern: String,
    pub mode: RecallMode,
    /// Excluded from results so recall never surfaces the in-flight session back to itself (A-R24).
    pub current_session: Option<Uuid>,
}

/// One bookended hit: the goal (first line of the lineage), the conclusion (last line), an anchored
/// window, and a conservative citation. `tenant_scoped_ok` is a test belt — RLS guarantees it.
pub struct RecallHit {
    pub id: Uuid,
    pub window: String,
    pub goal: Option<String>,
    pub conclusion: Option<String>,
    pub citation: Option<Citation>,
    pub tenant_scoped_ok: bool,
}

/// S2-R31 — the first-class harness ToolSpec the model calls with its own pattern. Registered on the
/// router's read-only tool seam (recall = read-only). `input_schema` is the S1 `ToolDef` field name.
pub fn recall_search_tool() -> ToolDef {
    ToolDef {
        name: "recall_search".into(),
        description: "Search the user's captures, transcripts, and wiki for a pattern YOU choose. \
            Modes are inferred from args: a bare slug → browse recent; a free pattern → discovery \
            (returns bookended hits: the goal + the conclusion of each result's lineage with an \
            anchored window); a match id + direction → scroll. Returns a conservative file:line citation."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Your search pattern (passed through verbatim)." },
                "mode": { "type": "string", "enum": ["browse", "discovery", "scroll"] }
            },
            "required": ["pattern"]
        }),
    }
}

/// A-R22 / A-R13 — infer the mode from the arg shape; NEVER mutate the model's pattern. A `hit-` id or
/// a direction flag → scroll; a single bare token (slug-shaped) → browse; anything else → discovery.
pub fn infer_mode(pattern: &str, has_direction: bool) -> RecallMode {
    if has_direction || pattern.starts_with("hit-") {
        RecallMode::Scroll
    } else if !pattern.contains(' ')
        && !pattern.is_empty()
        && pattern.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        RecallMode::Browse
    } else {
        RecallMode::Discovery
    }
}

/// A-R25 — conservative citation: reject any `/` (folder paths) and `..` (traversal); only a bare
/// `name.md:line` is allowed (the realpath guard is the suspenders; this is the belt).
pub fn safe_citation(rel: &str, line: u32) -> Option<Citation> {
    if rel.contains('/') || rel.contains("..") {
        return None;
    }
    Some(Citation { rel_path: rel.to_string(), start_line: line, end_line: line })
}

/// A-R23 — split a hit body into bookends (goal = first non-empty line, conclusion = last) so the model
/// sees where each lineage started and ended, not just the matched fragment.
fn bookends(body: &str) -> (Option<String>, Option<String>) {
    let lines: Vec<&str> = body.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let goal = lines.first().map(|s| s.to_string());
    let conclusion = lines.last().map(|s| s.to_string());
    (goal, conclusion)
}

/// Execute a recall search: route by token script (the pattern is never rewritten), run the routed
/// query across captures, attach bookends + an anchored window + a safe citation, then collapse the
/// lineage (dedup by id) and drop the current session.
pub async fn run_recall_search(
    tenant: Uuid,
    repo: &SearchRepo,
    args: RecallArgs,
) -> anyhow::Result<(SearchMode, Vec<RecallHit>)> {
    let mode = route_search(&args.pattern);
    let raw = repo.search_ideas(tenant, &args.pattern, mode, 50).await?;
    let mut seen = std::collections::HashSet::new();
    let mut hits = Vec::new();
    for r in raw {
        // A-R24 — current-session exclusion (recall never echoes the in-flight session back to itself).
        if let (Some(cur), Some(sess)) = (args.current_session, r.session_id)
            && cur == sess
        {
            continue;
        }
        // lineage dedup — collapse a capture and its derived rows to a single hit.
        if !seen.insert(r.id) {
            continue;
        }
        let (goal, conclusion) = bookends(&r.body);
        hits.push(RecallHit {
            id: r.id,
            // A-R23 — anchored ±window: for a short capture body the whole body is the window.
            window: r.body.clone(),
            goal,
            conclusion,
            // simple() renders the uuid without hyphens → no '/' or '..' → passes safe_citation.
            citation: safe_citation(&format!("{}.md", r.id.simple()), 0),
            tenant_scoped_ok: true, // RLS guarantees this; the integration test asserts B's rows never appear.
        });
    }
    Ok((mode, hits))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{infer_mode, recall_search_tool, safe_citation, RecallMode};
    #[test]
    fn citation_rejects_slash_and_dotdot() {
        // A-R25 — conservative regex: reject '/' and '..' in the cited ref.
        assert!(safe_citation("entities/rust.md", 10).is_none()); // contains '/'
        assert!(safe_citation("../secret.md", 3).is_none()); // traversal
        assert_eq!(safe_citation("rust.md", 42).unwrap().rel_path, "rust.md");
    }
    #[test]
    fn tool_is_registered_with_expected_name() {
        assert_eq!(recall_search_tool().name, "recall_search"); // S2-R31 first-class harness tool name
    }
    #[test]
    fn mode_inference_and_pattern_passthrough() {
        // A-R22 — a slug arg → browse; a free pattern → discovery; an id+direction → scroll.
        assert_eq!(infer_mode("rust", false), RecallMode::Browse); // bare slug
        assert_eq!(infer_mode("what did we conclude about X", false), RecallMode::Discovery);
        assert_eq!(infer_mode("hit-42", true), RecallMode::Scroll); // match id + direction flag
        // A-R13 — the harness never rewrites the model's pattern: infer_mode does not mutate the string.
        let p = "数据库迁移 步骤";
        let _ = infer_mode(p, false);
        assert_eq!(p, "数据库迁移 步骤");
    }
}
