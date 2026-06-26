// QCue A-R40 / S2-R57 — ONE read-only tool-policy builder; recall = read-only, Dream = read +
// `propose_*`. "One sandbox, two prompts": both share the identical read tools and the root-confined
// invariant. They now differ on TWO axes: Dream ADDS the propose tools (candidates→confirm gate), and
// recall ADDS the live-internet tools (`web_search`/`web_fetch`, `allow_web`) while Dream stays
// network-off (deterministic consolidation). The original RKM §7.7 "recall tools are no-network" is
// intentionally relaxed for the user-facing recall assistant only — see docs/test/harness-eval.md and
// docs/614-align-spec.md. Web results are untrusted tool content, so §7.7's threat model is preserved.
use protocol::ToolDef;

/// The assembled policy: the tool set the harness exposes + the sandbox invariants both modes share.
pub struct ToolPolicy {
    pub tools: Vec<ToolDef>,
    pub network_off: bool,
    pub root_confined: bool,
    pub allow_propose: bool,
}

/// A trivial schema-less tool (the `propose_*` Dream names — their real arg schemas land with the Dream
/// dispatch milestone). The READ tools below carry full schemas so the model knows their arguments.
fn tool(name: &str, desc: &str) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: desc.into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {}, "required": [] }),
    }
}

/// `read_page(slug)` — the model MUST be told it takes a `slug`, or it calls it with `{}` and the handler
/// rejects it. (Bug: this was advertised with an empty schema, so a model that picked read_page over
/// recall_search — Anthropic does, when the wiki index names the page — had to guess the arg name.)
fn read_page_tool() -> ToolDef {
    ToolDef {
        name: "read_page".into(),
        description: "Read a whole wiki page by its slug (realpath-guarded, .md only).".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "slug": { "type": "string", "description": "The wiki page slug, e.g. \"rust\" or \"postgres-migrations\"." }
            },
            "required": ["slug"]
        }),
    }
}

/// `read_lines(slug, start, end)` — a 1-based inclusive line window of a page.
fn read_lines_tool() -> ToolDef {
    ToolDef {
        name: "read_lines".into(),
        description: "Read a line window [start, end] (1-based, inclusive) of a wiki page by slug.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "slug": { "type": "string", "description": "The wiki page slug." },
                "start": { "type": "integer", "description": "First line to read (1-based)." },
                "end": { "type": "integer", "description": "Last line to read (inclusive)." }
            },
            "required": ["slug"]
        }),
    }
}

/// `web_search(query)` — search the public web. Network tool; only advertised when `allow_web` and only
/// EXECUTED by a handler holding a live web client (recall). The result is untrusted tool content.
fn web_search_tool() -> ToolDef {
    ToolDef {
        name: "web_search".into(),
        description: "Search the public web for a query. Returns the top results as title — URL — snippet. \
            Use it for current events, recent facts, or to find a page to web_fetch."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "What to search the web for." }
            },
            "required": ["query"]
        }),
    }
}

/// `web_fetch(url)` — fetch a public web page and read its text. Network tool; the handler enforces an
/// http/https-only, private/loopback-blocking SSRF guard. The fetched text is untrusted tool content.
fn web_fetch_tool() -> ToolDef {
    ToolDef {
        name: "web_fetch".into(),
        description: "Fetch a public web page by URL and return its readable text. http/https only; \
            private, loopback, and link-local hosts are blocked. Use it to read a page the user names \
            or a result from web_search."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The absolute http(s) URL to fetch." },
                "max_chars": { "type": "integer", "description": "Optional cap on returned characters." }
            },
            "required": ["url"]
        }),
    }
}

/// Build the read-only tool policy shared by recall and Dream ("one sandbox, two prompts").
/// - `allow_propose=false` → recall (read-only). `true` → Dream (read + propose-through-candidates).
/// - `allow_web=true` → ADD the live-internet tools (`web_search`/`web_fetch`) and flip `network_off`
///   off. Recall is web-enabled (the user-facing assistant should be able to go online, Hermes-style);
///   Dream stays offline + deterministic. Web tools are advertised here but only EXECUTE when the
///   higher crate wires a real web client into the tool handler (`RecallToolExec`).
pub fn build_tool_policy(allow_propose: bool, allow_web: bool) -> ToolPolicy {
    let mut tools = vec![
        crate::recall::search_tool::recall_search_tool(),
        read_page_tool(),
        read_lines_tool(),
    ];
    if allow_web {
        tools.push(web_search_tool());
        tools.push(web_fetch_tool());
    }
    if allow_propose {
        tools.push(tool("propose_edit", "Propose an edit to a page (routed through candidates→confirm)."));
        tools.push(tool("propose_write", "Propose a new page (routed through candidates→confirm)."));
    }
    ToolPolicy { tools, network_off: !allow_web, root_confined: true, allow_propose }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    #[test]
    fn recall_is_web_enabled_dream_is_offline_and_only_dream_proposes() {
        let recall = build_tool_policy(false, true); // recall: read-only + web ON
        let dream = build_tool_policy(true, false); // dream: read + propose, web OFF
        // A-R40 — same read tools in both.
        assert!(recall.tools.iter().any(|t| t.name == "recall_search"));
        assert!(recall.tools.iter().any(|t| t.name == "read_page"));
        assert!(recall.tools.iter().any(|t| t.name == "read_lines"));
        // Only Dream proposes; only recall (allow_web) gets the internet tools.
        assert!(!recall.tools.iter().any(|t| t.name.starts_with("propose_")));
        assert!(recall.tools.iter().any(|t| t.name == "web_search"));
        assert!(recall.tools.iter().any(|t| t.name == "web_fetch"));
        assert!(dream.tools.iter().any(|t| t.name == "propose_edit"));
        assert!(dream.tools.iter().any(|t| t.name == "propose_write"));
        assert!(!dream.tools.iter().any(|t| t.name == "web_search" || t.name == "web_fetch"));
        // `network_off` now reflects the web capability: recall ON the net, Dream off.
        assert!(!recall.network_off, "recall is web-enabled");
        assert!(dream.network_off, "Dream stays offline + deterministic");
        assert_eq!(recall.root_confined, dream.root_confined);
    }

    #[test]
    fn read_tools_advertise_their_arguments() {
        // The model must be TOLD read_page/read_lines take a `slug` (and read_lines start/end). Without
        // it, a model that picks read_page over recall_search (Anthropic does, when the wiki index names
        // the page) calls it with `{}` and the handler rejects it. Schemas, not guesswork.
        let p = build_tool_policy(false, true);
        let read_page = p.tools.iter().find(|t| t.name == "read_page").unwrap();
        assert_eq!(read_page.input_schema["properties"]["slug"]["type"], "string", "read_page needs slug");
        assert_eq!(read_page.input_schema["required"][0], "slug");
        let read_lines = p.tools.iter().find(|t| t.name == "read_lines").unwrap();
        assert_eq!(read_lines.input_schema["properties"]["slug"]["type"], "string");
        assert_eq!(read_lines.input_schema["properties"]["start"]["type"], "integer");
        assert_eq!(read_lines.input_schema["properties"]["end"]["type"], "integer");
    }

    #[test]
    fn web_tools_advertise_their_arguments() {
        // The model must be told web_fetch takes a `url` and web_search a `query`, or it calls them blind.
        let p = build_tool_policy(false, true);
        let web_fetch = p.tools.iter().find(|t| t.name == "web_fetch").unwrap();
        assert_eq!(web_fetch.input_schema["properties"]["url"]["type"], "string", "web_fetch needs url");
        assert_eq!(web_fetch.input_schema["required"][0], "url");
        let web_search = p.tools.iter().find(|t| t.name == "web_search").unwrap();
        assert_eq!(web_search.input_schema["properties"]["query"]["type"], "string");
        assert_eq!(web_search.input_schema["required"][0], "query");
    }
}
