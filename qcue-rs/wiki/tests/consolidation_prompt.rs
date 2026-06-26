// QCue S2-R58 / A-R17 — the 4-phase Dream prompt is a golden file; byte-drift is the regression.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use wiki::prompts::consolidation::build_consolidation_prompt;

#[test]
fn consolidation_prompt_golden() {
    let prompt = build_consolidation_prompt("Sessions since: s1, s2");
    let golden = include_str!("golden/consolidation_prompt.txt");
    assert_eq!(prompt, golden); // byte-equal (the prompt IS the spec)

    // structural assertions (defense for future edits to the golden)
    assert!(prompt.contains("Phase 1 — Orient"));
    assert!(prompt.contains("Phase 2 — Gather"));
    assert!(prompt.contains("Phase 3 — Consolidate"));
    assert!(prompt.contains("Phase 4 — Prune"));
    assert!(prompt.contains("index.md"));
    assert!(prompt.contains("200 lines") && prompt.contains("25KB") && prompt.contains("150 char"));
    assert!(prompt.contains("demote") && prompt.contains("200 char"));
    assert!(
        prompt.contains("recall_search")
            && prompt.contains("read_page")
            && prompt.contains("read_lines")
    );
    assert!(prompt.contains("absolute dates"));
    assert!(prompt.contains("resolve contradictions"));
    assert!(prompt.contains("entities/") && prompt.contains("concepts/") && prompt.contains("sources/"));
}
