//! QCue SBX-R3 — the shared SSRF egress guard. One source of truth for "is this address/URL a PUBLIC
//! unicast target", used by BOTH the recall `web_fetch`/`web_search` path AND the provider dispatch
//! client (so a custom OpenAI-compatible `base_url` can never point the server at loopback / RFC1918 /
//! link-local / CGNAT / cloud-metadata). PURE except `GuardedResolver` (DNS at connect time).
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Block any address that is NOT a normal PUBLIC unicast address (the core SSRF defense).
pub fn ip_is_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_blocked(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return v4_blocked(mapped);
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // unique-local fc00::/7
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
        }
    }
}

fn v4_blocked(v4: Ipv4Addr) -> bool {
    let o = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_documentation()
        || v4.is_multicast()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xc0) == 0x40) // CGNAT 100.64.0.0/10
}

/// True iff `host` is an IP literal in a blocked range, OR an obvious internal name. Used by the base_url
/// validator (which has only the string, no live resolution). Hostname targets are additionally guarded
/// at connect time by [`GuardedResolver`].
pub fn host_is_blocked_literal(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = h.parse::<IpAddr>() {
        return ip_is_blocked(ip);
    }
    let h = host.to_ascii_lowercase();
    h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h == "metadata.google.internal"
}

/// Validate a model-authored URL: http/https only, host present, and any IP-literal host must be public.
pub fn classify_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| format!("not a valid absolute URL: {raw}"))?;
    match url.scheme() {
        "http" | "https" => {}
        other => return Err(format!("blocked URL scheme `{other}` (only http/https are allowed)")),
    }
    let host = url.host_str().ok_or_else(|| "URL has no host".to_string())?;
    let host_ip = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host_ip.parse::<IpAddr>() {
        if ip_is_blocked(ip) {
            return Err(format!("blocked non-public address: {host}"));
        }
        return Ok(url);
    }
    let h = host.to_ascii_lowercase();
    if h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h == "metadata.google.internal"
    {
        return Err(format!("blocked internal host: {host}"));
    }
    Ok(url)
}

/// A reqwest resolver that drops any resolved address that is NOT public — so a hostname that resolves
/// to a private/loopback/metadata IP (incl. via a redirect hop) cannot be connected to (DNS-rebinding).
pub struct GuardedResolver;
impl reqwest::dns::Resolve for GuardedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let host = name.as_str().to_owned();
            let addrs = tokio::net::lookup_host((host.as_str(), 0u16))
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
            let ok: Vec<SocketAddr> = addrs.filter(|sa| !ip_is_blocked(sa.ip())).collect();
            if ok.is_empty() {
                return Err::<reqwest::dns::Addrs, Box<dyn std::error::Error + Send + Sync>>(
                    format!("blocked host {host}: resolves only to non-public addresses").into(),
                );
            }
            Ok(Box::new(ok.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classify_blocks_ssrf_targets() {
        for b in [
            "http://localhost/", "http://127.0.0.1/", "http://10.0.0.5/admin",
            "http://172.16.9.9/", "http://192.168.1.1/", "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/", "http://0.0.0.0/", "http://100.64.1.1/",
            "https://metadata.google.internal/", "http://foo.local/", "ftp://example.com/",
            "file:///etc/passwd", "not-a-url",
        ] {
            assert!(classify_url(b).is_err(), "must BLOCK {b}");
        }
    }
    #[test]
    fn classify_allows_public_web() {
        for g in ["https://example.com/", "https://api.openai.com/v1", "https://1.1.1.1/"] {
            assert!(classify_url(g).is_ok(), "must ALLOW {g}");
        }
    }
    #[test]
    fn host_literal_block_matches_ranges() {
        assert!(host_is_blocked_literal("169.254.169.254"));
        assert!(host_is_blocked_literal("10.0.0.5"));
        assert!(host_is_blocked_literal("localhost"));
        assert!(!host_is_blocked_literal("api.openai.com"));
        assert!(!host_is_blocked_literal("1.1.1.1"));
    }
}
