// QCue S2-R58 / A-R17 — the verbatim 4-phase Dream consolidation prompt (Orient → Gather → Consolidate
// → Prune + index), retargeted from Claude's MEMORY.md/topic-files to the Karpathy wiki layout
// (entities/ concepts/ sources/ index.md log.md + daily logs logs/YYYY/MM/DD.md). The prompt IS the
// spec: a golden-file test asserts byte-equality (drift is a regression). Phase-4 index discipline
// mirrors `memdir.ts` (≤200 lines AND ≤25KB, one line ≤150 chars, demote >200-char lines, resolve
// contradictions). The narrow-grep instruction names the read-only recall surface
// (recall_search/read_page/read_lines) — the SAME S1 search capability recall uses (App. A §3.1).
use crate::prompts::constraints::UNIVERSAL_LINK_CONSTRAINTS;

/// Build the 4-phase Dream prompt. `extra` is the read-only tool note + the sessions-since list,
/// appended under `## Additional context` (Claude appends the tool constraint there, not in the body,
/// because a manual /dream runs with normal permissions where it would mislead — autoDream.ts:214-221).
pub fn build_consolidation_prompt(extra: &str) -> String {
    format!(
        "You are performing a dream — a reflective pass over your wiki. Synthesize what you've learned recently \
into durable, well-organized pages so future sessions can orient quickly.\n\n\
The wiki layout is: entities/ concepts/ sources/ index.md log.md, plus daily logs logs/YYYY/MM/DD.md.\n\n\
## Phase 1 — Orient (read-only)\n\
- ls the wiki root; read index.md to understand the current catalog.\n\
- Skim existing entity/concept/source pages so you improve them rather than creating duplicates.\n\
- Review recent logs/YYYY/MM/* entries.\n\n\
## Phase 2 — Gather recent signal (read-only)\n\
- Scan daily logs + new captures since the clock; note facts that drifted or contradict what's now true.\n\
- Use the recall_search tool with narrow terms YOU choose (then read_page / read_lines to follow up) \
over the capture-log/transcript JSONL. Don't exhaustively read; look only for things you already suspect matter.\n\n\
## Phase 3 — Consolidate (propose writes)\n\
- Merge new signal into existing pages rather than near-duplicates.\n\
- Convert relative dates (\"yesterday\") to absolute dates so they stay interpretable.\n\
- Delete/fix contradicted facts — fix at the source page.\n{UNIVERSAL_LINK_CONSTRAINTS}\n\n\
## Phase 4 — Prune and index (propose writes)\n\
- Rewrite index.md so it stays under 200 lines AND under ~25KB. It's an index, not a dump — \
one line per entry under ~150 characters: `- [[Title]] — one-line hook`.\n\
- Remove stale/superseded pointers; demote any index line over ~200 characters into the topic page.\n\
- Add pointers to newly important pages; resolve contradictions by fixing the wrong page.\n\
- Append a log.md / daily-log entry of what changed.\n\n\
Return a brief summary of what you consolidated, updated, or pruned. If nothing changed, say so.\n\n\
## Additional context\n{extra}\n"
    )
}
