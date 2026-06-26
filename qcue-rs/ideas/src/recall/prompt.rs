// QCue — the AGENTIC recall system prompt (Appendix A: recall is agentic, NOT a closed-book retrieval).
//
// The bug this replaces: the user-facing recall/chat stream drove `wiki::build_synthesis_prompt`, whose
// rule #1 is literally "answer from the wiki, not general knowledge". That turns a capable model into a
// RAG bot that refuses everything outside the user's notes — the opposite of the harness goal (maximize
// each provider's capability, Hermes-style).
//
// The fix: keep the model's FULL general knowledge and hand it the user's second brain as TOOLS
// (`recall_search`/`read_page`/`read_lines`, already advertised+executed by `RouterWikiLlm::live_recall`).
// The model answers general questions directly and reaches for the tools only when the question is about
// the user's own captures/notes. The wiki index rides along as a table-of-contents hint, NOT a cage.
use wiki::prompts::constraints::UNIVERSAL_LINK_CONSTRAINTS;

/// Build the recall system prefix. `prefer_wiki=true` is the explicit "wiki query" surface (prioritise +
/// always cite the user's pages); `false` is the general assistant. NEITHER forbids general knowledge.
///
/// `provider`/`model` are the REAL provider display-name + model id the harness resolved for THIS tenant
/// (e.g. `"DeepSeek"` / `"deepseek-v4-pro"`). They are injected as GROUND TRUTH so the model answers
/// "which model are you?" truthfully and specifically — the harness goal is to MAXIMIZE capability and
/// transparency, not to hide the model behind a vague "your configured model". Anchoring to the truth
/// ALSO fixes the old hallucination (a DeepSeek key answering "I am Claude"): we no longer merely forbid
/// naming a vendor, we tell the model exactly which model/provider it actually runs on.
///
/// `allow_web=true` advertises the live-internet tools (`web_search`/`web_fetch`); it must match the
/// tool set the harness actually wires (`build_tool_policy(_, allow_web)`), or the model would be told
/// about tools it cannot call.
pub fn build_recall_prompt(
    index: &str,
    prefer_wiki: bool,
    provider: &str,
    model: &str,
    allow_web: bool,
) -> String {
    let focus = if prefer_wiki {
        "This is a WIKI QUERY: prioritise the user's wiki when it is relevant, and always cite the pages \
         you draw on. You may still use your general knowledge to explain, connect ideas, and fill gaps \
         the wiki does not cover."
    } else {
        "Answer general-knowledge questions directly from what you know — do NOT force a search and do NOT \
         restrict yourself to the wiki. Reach for the tools when (and only when) the question is about the \
         user's own notes, ideas, projects, captures, or past conclusions."
    };
    // The web tools are advertised ONLY when the harness actually wired them (allow_web). The wording is
    // still byte-stable for a given (provider, model, allow_web), so the system prefix rides the cache.
    let web_tools = if allow_web {
        "You also have LIVE INTERNET access — use it whenever the question is about current events, \
         recent facts, anything that may have changed since your training, or a URL the user gives you:\n\
         - web_search(query): search the public web; returns titles, URLs, and snippets you can follow up.\n\
         - web_fetch(url): fetch a public web page and read its text (http/https only).\n"
    } else {
        ""
    };
    format!(
        "You are QCue, the user's personal assistant. You have your FULL general knowledge and answer any \
         question to the best of your ability — you are NOT limited to the user's notes.\n\n\
         Your identity: you are running on the model `{model}`, served by {provider}, configured by the \
         user with their own API key (BYOK). When the user asks which model, provider, AI, or company you \
         are, answer truthfully and specifically — name the model (`{model}`) and the provider ({provider}). \
         Do not be vague or evasive about it. You must NOT claim to be a DIFFERENT vendor's model than the \
         one named here.\n\n\
         The user keeps a personal \"second brain\": a wiki distilled from their captured ideas. You can \
         consult it with tools:\n\
         - recall_search(pattern): full-text search the user's captures, transcripts, and wiki.\n\
         - read_page(slug): read a whole wiki page.\n\
         - read_lines(slug, start, end): read a line window of a page.\n\
         {web_tools}\n\
         {focus}\n\n\
         When you use a wiki page, cite it inline as [[slug]] and end with a `## References` section listing \
         each page you used as [[slug|Display]] — short description. Do not add a References section when you \
         did not use any page.\n\
         {UNIVERSAL_LINK_CONSTRAINTS}\n\n\
         The user's wiki index (a table of contents — use the tools to read the actual content):\n{index}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_prompt_grants_general_knowledge_and_advertises_web_tools() {
        let p = build_recall_prompt("# Index\n\n(wiki is empty)\n", false, "DeepSeek", "deepseek-v4-pro", true);
        let lc = p.to_lowercase();
        // The regression guard: the old closed-book wording must NEVER come back.
        assert!(!lc.contains("not general knowledge"), "must not forbid general knowledge:\n{p}");
        assert!(!p.contains("Answer ONLY from the wiki"), "must not be closed-book:\n{p}");
        // It positively GRANTS general knowledge and advertises the agentic tools.
        assert!(lc.contains("general knowledge"));
        assert!(lc.contains("not limited"));
        assert!(p.contains("recall_search") && p.contains("read_page") && p.contains("read_lines"));
        // allow_web=true advertises the live-internet tools so the model knows it can go online.
        assert!(p.contains("web_search") && p.contains("web_fetch"), "web tools must be advertised:\n{p}");
        assert!(lc.contains("internet"), "must tell the model it can reach the internet:\n{p}");
        // The index rides along as reference context.
        assert!(p.contains("(wiki is empty)"));
        // Link discipline is still injected (the markdown-side belt; S2-R50).
        assert!(p.contains("[[wikilinks]]"));
    }

    #[test]
    fn recall_prompt_states_the_real_provider_and_model_truthfully() {
        // The fix: a BYOK tenant must be able to learn WHICH model/provider they configured. The old
        // prompt forbade naming any vendor (answering vaguely "your configured model"), which both
        // (a) hid the truth the user wanted and (b) still let DeepSeek hallucinate "I am Claude". We
        // now inject the REAL resolved (provider, model) as ground truth: truthful + specific, and it
        // anchors identity to the truth instead of a blanket "never name a vendor".
        let ds = build_recall_prompt("# Index\n", false, "DeepSeek", "deepseek-v4-pro", true);
        assert!(ds.contains("deepseek-v4-pro"), "must name the real model id:\n{ds}");
        assert!(ds.contains("DeepSeek"), "must name the real provider:\n{ds}");
        let lc = ds.to_lowercase();
        assert!(lc.contains("truthfully"), "must instruct a truthful, specific answer:\n{ds}");
        // It still forbids impersonating a DIFFERENT vendor than the one actually running.
        assert!(lc.contains("different vendor"), "must still forbid impersonating another vendor:\n{ds}");
        // It must NOT revert to the old vague evasion.
        assert!(!lc.contains("never claim to be claude"), "must not bring back the blanket gag:\n{ds}");

        // Parameterized: an Anthropic tenant sees Anthropic/Claude, not DeepSeek.
        let an = build_recall_prompt("# Index\n", false, "Anthropic", "claude-opus-4-8", true);
        assert!(an.contains("claude-opus-4-8") && an.contains("Anthropic"));
        assert!(!an.contains("deepseek-v4-pro"), "identity is the ACTUAL route, not a fixed string");
    }

    #[test]
    fn wiki_query_variant_emphasises_citing_and_can_omit_web_tools() {
        // allow_web=false → the no-internet variant must NOT advertise web tools it cannot call.
        let p = build_recall_prompt("# Index\n- [[rust|Rust]] — the language\n", true, "OpenAI", "gpt-5.5", false);
        let lc = p.to_lowercase();
        assert!(p.contains("## References"));
        assert!(lc.contains("cite"));
        assert!(lc.contains("prioritise the user's wiki"));
        // even the wiki-focused surface keeps the model's general knowledge available.
        assert!(!lc.contains("not general knowledge"));
        assert!(lc.contains("general knowledge"));
        // allow_web=false must not advertise web tools.
        assert!(!p.contains("web_search") && !p.contains("web_fetch"), "no web tools when allow_web=false:\n{p}");
        // identity is still present and truthful.
        assert!(p.contains("gpt-5.5") && p.contains("OpenAI"));
    }
}
