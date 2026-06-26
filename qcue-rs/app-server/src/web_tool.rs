//! QCue — the LIVE-INTERNET tool executor for the agentic recall harness (`web_fetch` / `web_search`).
//!
//! The recall harness advertises `web_fetch`/`web_search` (ideas::recall::tool_policy with `allow_web`);
//! `RecallToolExec` routes the model's call here. This is where QCue makes an OUTBOUND request on the
//! model's behalf, so it applies the shared SSRF guard (`http::ssrf`, also used by the provider dispatch
//! client): http/https only, and the target host — whether an IP literal OR a resolved hostname (every
//! connection passes through `GuardedResolver`) — must be a PUBLIC unicast address. Private / loopback /
//! link-local / CGNAT / cloud-metadata ranges are refused, and each
//! redirect hop is re-validated. The fetched text is returned as UNTRUSTED tool content (the dispatcher
//! marks every tool result untrusted, S1-R38 / pitfall #1), so web prompt-injection is contained by the
//! existing untrusted-message-tail handling — web content never enters the byte-stable system prefix.
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_MAX_CHARS: usize = 8_000;
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024; // never read more than 2 MiB of a single page
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
const UA: &str = "Mozilla/5.0 (compatible; QCueBot/0.1; +https://qcue.cn)";

use http::ssrf::{classify_url, GuardedResolver};

// ─────────────────────────────────────────────────────────────────────────────────────────────────────
// HTML → text + DuckDuckGo result parsing (PURE, unit-tested).
// ─────────────────────────────────────────────────────────────────────────────────────────────────────

/// Strip HTML to readable text: drop `<script>/<style>/<head>/<noscript>` blocks, turn tags into spaces,
/// decode a handful of entities, collapse whitespace.
pub fn html_to_text(html: &str) -> String {
    let cleaned = strip_blocks(html, &["script", "style", "head", "noscript"]);
    let mut out = String::with_capacity(cleaned.len());
    let mut in_tag = false;
    for ch in cleaned.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            _ if in_tag => {}
            _ => out.push(ch),
        }
    }
    collapse_ws(&decode_entities(&out))
}

/// Remove `<tag ...>...</tag>` blocks (case-insensitive), replacing each with a space.
fn strip_blocks(html: &str, tags: &[&str]) -> String {
    let mut s = html.to_string();
    for tag in tags {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        loop {
            let lower = s.to_ascii_lowercase();
            let Some(start) = lower.find(&open) else { break };
            let end = lower[start..]
                .find(&close)
                .map(|r| start + r + close.len())
                .unwrap_or(s.len());
            s.replace_range(start..end, " ");
        }
    }
    s
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&") // &amp; LAST so "&amp;lt;" → "&lt;", not "<"
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// One web_search hit.
struct SearchHit {
    title: String,
    url: String,
    snippet: String,
}

/// Parse DuckDuckGo HTML results (`html.duckduckgo.com/html`). Best-effort: returns `[]` if the markup
/// shifts (the caller then falls back to the stripped page text), so a layout change degrades, not breaks.
fn parse_ddg_results(html: &str) -> Vec<SearchHit> {
    let anchors = extract_anchors(html, "result__a");
    let snippets = extract_class_texts(html, "result__snippet");
    let mut hits = Vec::new();
    for (i, (href, title)) in anchors.into_iter().enumerate() {
        let url = ddg_unwrap(&href);
        if url.is_empty() {
            continue;
        }
        let snippet = snippets.get(i).map(|s| html_to_text(s)).unwrap_or_default();
        hits.push(SearchHit { title: html_to_text(&title), url, snippet });
    }
    hits
}

/// Extract `(href, inner_text)` for every `<a …class="…<marker>…">…</a>`.
fn extract_anchors(html: &str, class_marker: &str) -> Vec<(String, String)> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = lower[cursor..].find("<a ") {
        let tag_start = cursor + rel;
        let Some(rel_gt) = lower[tag_start..].find('>') else { break };
        let tag_end = tag_start + rel_gt; // index of '>'
        let tag = &html[tag_start..=tag_end];
        let tag_lower = &lower[tag_start..=tag_end];
        let inner_start = tag_end + 1;
        let close = lower[inner_start..].find("</a>").map(|r| inner_start + r);
        if tag_lower.contains(class_marker)
            && let Some(href) = attr_value(tag, "href")
        {
            let inner = match close {
                Some(c) => &html[inner_start..c],
                None => "",
            };
            out.push((href, inner.to_string()));
        }
        cursor = close.map(|c| c + 4).unwrap_or(tag_end + 1);
    }
    out
}

/// Extract the inner text of every element carrying `class="…<marker>…"` (snippets live on `<a>`/`<div>`).
fn extract_class_texts(html: &str, class_marker: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = lower[cursor..].find(class_marker) {
        let at = cursor + rel;
        if let Some(rel_gt) = lower[at..].find('>') {
            let gt = at + rel_gt + 1;
            if let Some(rel_close) = lower[gt..].find("</") {
                let close = gt + rel_close;
                out.push(html[gt..close].to_string());
                cursor = close + 2;
                continue;
            }
        }
        cursor = at + class_marker.len();
    }
    out
}

fn attr_value(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let pat = format!("{name}=\"");
    let i = lower.find(&pat)? + pat.len();
    let rest = &tag[i..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// DDG result hrefs are redirect links (`//duckduckgo.com/l/?uddg=<encoded real url>`); unwrap to the real
/// URL. A direct `http(s)://…` href is returned as-is; anything else yields "" (skipped).
fn ddg_unwrap(href: &str) -> String {
    let abs = if let Some(rest) = href.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        href.to_string()
    };
    if let Ok(url) = reqwest::Url::parse(&abs)
        && let Some((_, v)) = url.query_pairs().find(|(k, _)| k == "uddg")
    {
        return v.into_owned();
    }
    if href.starts_with("http") {
        href.to_string()
    } else {
        String::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────────
// The network client.
// ─────────────────────────────────────────────────────────────────────────────────────────────────────

/// The live web client used by the recall tool handler.
pub struct WebClient {
    client: reqwest::Client,
}

impl Default for WebClient {
    fn default() -> Self {
        Self::new()
    }
}

impl WebClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(UA)
            .timeout(FETCH_TIMEOUT)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    return attempt.error("too many redirects");
                }
                // Re-validate every hop (scheme + IP-literal range); name hops are re-guarded by the resolver.
                match classify_url(attempt.url().as_str()) {
                    Ok(_) => attempt.follow(),
                    Err(_) => attempt.stop(),
                }
            }))
            .dns_resolver(Arc::new(GuardedResolver))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    /// `web_fetch` — fetch a URL the model authored and return its readable text (SSRF-guarded, capped).
    pub async fn fetch(&self, arguments: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(arguments).map_err(|e| format!("invalid web_fetch arguments: {e}"))?;
        let raw = v.get("url").and_then(|u| u.as_str()).unwrap_or("").trim();
        if raw.is_empty() {
            return Err("web_fetch requires a `url`".into());
        }
        let max_chars = v
            .get("max_chars")
            .and_then(|n| n.as_u64())
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MAX_CHARS)
            .clamp(200, 20_000);
        let url = classify_url(raw)?;
        let resp = self
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| format!("web_fetch failed: {e}"))?;
        let status = resp.status();
        let ctype = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = read_capped(resp).await?;
        let looks_html = ctype.contains("html") || body.trim_start().starts_with('<');
        let text = if looks_html { html_to_text(&body) } else { collapse_ws(&body) };
        Ok(format!("Fetched {url} (HTTP {}):\n\n{}", status.as_u16(), truncate_chars(&text, max_chars)))
    }

    /// `web_search` — search the public web via DuckDuckGo HTML and render the top hits for the model.
    pub async fn search(&self, arguments: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(arguments).map_err(|e| format!("invalid web_search arguments: {e}"))?;
        let query = v.get("query").and_then(|q| q.as_str()).unwrap_or("").trim().to_string();
        if query.is_empty() {
            return Err("web_search requires a `query`".into());
        }
        let url = reqwest::Url::parse_with_params(
            "https://html.duckduckgo.com/html/",
            &[("q", query.as_str())],
        )
        .map_err(|e| e.to_string())?;
        let url = classify_url(url.as_str())?;
        let resp =
            self.client.get(url).send().await.map_err(|e| format!("web_search failed: {e}"))?;
        let body = read_capped(resp).await?;
        let hits = parse_ddg_results(&body);
        if hits.is_empty() {
            let text = truncate_chars(&html_to_text(&body), 1500);
            return Ok(format!("No structured results parsed for \"{query}\". Page text:\n{text}"));
        }
        let mut out = format!("Top web results for \"{query}\":\n");
        for (i, h) in hits.iter().take(6).enumerate() {
            out.push_str(&format!(
                "\n[{}] {}\n    {}\n    {}\n",
                i + 1,
                truncate_chars(&h.title, 200),
                h.url,
                truncate_chars(&h.snippet, 300)
            ));
        }
        Ok(out)
    }
}

/// Read a response body, hard-capping the bytes read so a giant page can't blow memory.
async fn read_capped(resp: reqwest::Response) -> Result<String, String> {
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("web read failed: {e}"))?;
        if buf.len() + chunk.len() > MAX_BODY_BYTES {
            let take = MAX_BODY_BYTES.saturating_sub(buf.len());
            buf.extend_from_slice(&chunk[..take.min(chunk.len())]);
            break;
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn html_to_text_strips_tags_scripts_and_decodes_entities() {
        let html = "<html><head><style>.x{color:red}</style></head>\
            <body><h1>Hello</h1><p>A &amp; B &lt;ok&gt;</p>\
            <script>alert('bad')</script></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Hello"), "kept heading: {text}");
        assert!(text.contains("A & B <ok>"), "decoded entities: {text}");
        assert!(!text.contains("alert"), "dropped <script>: {text}");
        assert!(!text.contains("color:red"), "dropped <style>: {text}");
    }

    #[test]
    fn ddg_results_are_parsed_and_redirect_unwrapped() {
        let html = r#"<div class="result results_links">
            <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org%2F&amp;rut=abc">The Rust Programming Language</a>
            <a class="result__snippet" href="x">A language empowering <b>everyone</b>.</a>
          </div>"#;
        let hits = parse_ddg_results(html);
        assert_eq!(hits.len(), 1, "one result parsed");
        assert_eq!(hits[0].url, "https://www.rust-lang.org/", "uddg redirect unwrapped to the real URL");
        assert!(hits[0].title.contains("Rust Programming Language"), "title: {}", hits[0].title);
        assert!(hits[0].snippet.contains("empowering everyone"), "snippet (tags stripped): {}", hits[0].snippet);
    }
}
