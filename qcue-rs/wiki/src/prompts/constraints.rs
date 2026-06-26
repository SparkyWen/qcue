// QCue S2-R50 — UNIVERSAL_LINK_CONSTRAINTS (REUSE verbatim, constraints.ts:6-9) injected into EVERY
// link-emitting prompt. Centralizing the link rules keeps every generation/merge/synthesis prompt
// consistent (the markdown-side belt; the write-gate link-sanitizer is the suspenders, pitfall #11).
pub const UNIVERSAL_LINK_CONSTRAINTS: &str = "\
LINK RULES (mandatory):\n\
- Use [[wikilinks]] only — never HTML or markdown links.\n\
- Link by bare slug: [[rust]] or [[rust|Rust]]. Never write folder paths in link display text (no [[entities/rust|entities/rust]]).\n\
- Do not invent links to pages you have not been told exist.";

pub fn build_extraction_prompt(language: &str) -> String {
    format!("Extract entities and concepts. Output language: {language}. Names are NOT translated.\n{UNIVERSAL_LINK_CONSTRAINTS}")
}
pub fn build_page_generation_prompt(planned_paths: &[String]) -> String {
    format!("Write the page. Planned pages you may link: {}\n{UNIVERSAL_LINK_CONSTRAINTS}", planned_paths.join(", "))
}
pub fn build_merge_prompt() -> String {
    format!("Merge new info into the existing page. Return NO_NEW_CONTENT if nothing is new.\n{UNIVERSAL_LINK_CONSTRAINTS}")
}
pub fn build_synthesis_prompt(index: &str, loaded_titles: &[String]) -> String {
    format!(
        "Answer ONLY from the wiki below. Index:\n{index}\nLoaded pages: {}\n\
          Rules: (1) answer from the wiki, not general knowledge; (2) [[wikilinks]] only; \
          (3) end with a `## References` section citing every page used as [[path|Display]] — description.\n{UNIVERSAL_LINK_CONSTRAINTS}",
        loaded_titles.join(", ")
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    #[test]
    fn constraints_injected_into_every_link_emitting_prompt() {
        // S2-R50 — the constant appears in each builder: extraction/generation/merge/synthesis.
        assert!(build_extraction_prompt("en").contains(UNIVERSAL_LINK_CONSTRAINTS));
        assert!(build_page_generation_prompt(&[]).contains(UNIVERSAL_LINK_CONSTRAINTS));
        assert!(build_merge_prompt().contains(UNIVERSAL_LINK_CONSTRAINTS));
        assert!(build_synthesis_prompt("INDEX", &[]).contains(UNIVERSAL_LINK_CONSTRAINTS));
    }
    #[test]
    fn constraints_text_is_verbatim() {
        // golden: the plan's wording (drift is a regression).
        assert!(UNIVERSAL_LINK_CONSTRAINTS.contains("[[wikilinks]]"));
        assert!(UNIVERSAL_LINK_CONSTRAINTS.contains("Never write folder paths in link display text"));
    }
}
