// QCue S1-R15, S1-R16 — client builder, base_url security guard, versioned base_url, HTTP1 toggle.
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct ClientOpts {
    pub force_http1: bool,
    /// SBX-R4 — install the GuardedResolver so a hostname that resolves to a non-public IP (incl. via
    /// DNS rebinding) cannot be connected to. On in prod; off in dev/tests (loopback mock servers).
    pub guard_egress: bool,
}

#[derive(Debug)]
pub enum HttpBuildError {
    Insecure(String),
    Reqwest(String),
}
impl std::fmt::Display for HttpBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpBuildError::Insecure(u) => write!(f, "insecure base_url rejected: {u}"),
            HttpBuildError::Reqwest(e) => write!(f, "client build failed: {e}"),
        }
    }
}
impl std::error::Error for HttpBuildError {}

/// S1-R15 + SBX-R4 — refuse plain http:// except loopback; AND (in prod, `allow_insecure == false`)
/// refuse any host that is an IP literal in a blocked range or an internal name, so a custom
/// OpenAI-compatible base_url can never point the server at loopback / RFC1918 / link-local / CGNAT /
/// cloud-metadata. In dev/tests (`allow_insecure == true`) the loopback mock-server path is preserved.
pub fn validate_base_url_security(
    base_url: &str,
    allow_insecure: bool,
) -> Result<(), HttpBuildError> {
    // Dev/test escape hatch (loopback mock servers): skip the SSRF host check entirely.
    if allow_insecure {
        return Ok(());
    }
    // Parse with the SAME URL parser reqwest will actually use (NOT a hand-rolled split), so userinfo
    // (`https://x@169.254.169.254/`), ports, and alternate IPv4 encodings (decimal/octal/hex — the url
    // crate normalizes these for the special https scheme) can't smuggle a blocked host past the check.
    let url = reqwest::Url::parse(base_url).map_err(|_| HttpBuildError::Insecure(base_url.to_string()))?;
    match url.scheme() {
        "https" => {}
        // plain http is refused in prod regardless of host (was: loopback-only).
        _ => return Err(HttpBuildError::Insecure(format!("plain http not allowed: {base_url}"))),
    }
    let host = url.host_str().unwrap_or(""); // host_str() excludes userinfo + port.
    if host.is_empty() || crate::ssrf::host_is_blocked_literal(host) {
        return Err(HttpBuildError::Insecure(format!("blocked base_url host: {base_url}")));
    }
    Ok(())
}

/// S1-R15 — normalize the /v1 vs /beta vs bare-host mess; chat/completions always targets a /v1 path.
pub fn versioned_base_url(base_url: &str, path: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let normalized = if trimmed.ends_with("/beta") {
        trimmed.trim_end_matches("/beta").to_string() + "/v1"
    } else if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        trimmed.to_string() + "/v1"
    };
    format!("{normalized}/{}", path.trim_start_matches('/'))
}

/// S1-R16 — build the reqwest client; H2 with tuned keepalive by default, HTTP/1.1 escape hatch.
pub fn build_client(opts: ClientOpts) -> Result<reqwest::Client, HttpBuildError> {
    let mut b = reqwest::Client::builder()
        // S1-R38 / SBX-R4 — never follow redirects. LLM API POST endpoints never legitimately 3xx;
        // following one would re-attach the per-tenant auth header (the raw `x-api-key` reqwest does NOT
        // strip on a cross-host hop) to an attacker-chosen public host the egress guard can't catch.
        .redirect(reqwest::redirect::Policy::none())
        .tcp_keepalive(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(20));
    b = if opts.force_http1 {
        b.http1_only()
    } else {
        b.http2_keep_alive_interval(Duration::from_secs(15))
            .http2_keep_alive_while_idle(true)
    };
    if opts.guard_egress {
        b = b.dns_resolver(std::sync::Arc::new(crate::ssrf::GuardedResolver));
    }
    b.build().map_err(|e| HttpBuildError::Reqwest(e.to_string()))
}

/// Reads the `QCUE_FORCE_HTTP1` / `QCUE_ALLOW_INSECURE_HTTP` env toggles.
pub fn opts_from_env() -> (ClientOpts, bool) {
    let force_http1 = std::env::var("QCUE_FORCE_HTTP1").is_ok();
    let allow_insecure = std::env::var("QCUE_ALLOW_INSECURE_HTTP").is_ok();
    (ClientOpts { force_http1, guard_egress: !allow_insecure }, allow_insecure)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn base_url_guard_blocks_private_and_metadata_in_prod() {
        // allow_insecure = false (prod): an https base_url that targets an internal IP/name is rejected.
        for bad in [
            "https://169.254.169.254/v1",
            "https://10.0.0.5/v1",
            "http://192.168.1.10/v1",
            "https://metadata.google.internal/v1",
        ] {
            assert!(validate_base_url_security(bad, false).is_err(), "must BLOCK {bad}");
        }
        // public providers still pass
        for good in ["https://api.openai.com/v1", "https://api.deepseek.com"] {
            assert!(validate_base_url_security(good, false).is_ok(), "must ALLOW {good}");
        }
    }

    #[test]
    fn loopback_still_allowed_in_dev() {
        // allow_insecure = true (dev/tests with a loopback mock server): loopback is fine.
        assert!(validate_base_url_security("http://127.0.0.1:8080/v1", true).is_ok());
        assert!(validate_base_url_security("http://localhost:8080/v1", true).is_ok());
    }

    // S1-R38 / SBX-R4 — the provider client must NOT follow HTTP redirects. LLM POST endpoints never
    // legitimately 3xx; following a redirect would re-attach the per-tenant auth header (e.g. the raw
    // `x-api-key` Anthropic key, which reqwest does NOT strip — only `Authorization` is dropped on a
    // cross-host hop) to an attacker-chosen host. The egress guard only blocks private IPs, not a public
    // attacker host, so the redirect itself must be refused. A 3xx is surfaced to the caller verbatim.
    #[tokio::test]
    async fn build_client_does_not_follow_redirects() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let leaked = Arc::new(AtomicUsize::new(0));

        let leaked_srv = leaked.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let leaked_conn = leaked_srv.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .unwrap_or("")
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("");
                    if path == "/start" {
                        let _ = sock
                            .write_all(
                                b"HTTP/1.1 302 Found\r\nLocation: /leaked\r\nContent-Length: 0\r\n\r\n",
                            )
                            .await;
                    } else if path == "/leaked" {
                        // If the client followed the redirect, this counter is bumped → the auth header
                        // would have ridden along. It must stay 0.
                        leaked_conn.fetch_add(1, Ordering::SeqCst);
                        let _ = sock
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nleaked")
                            .await;
                    }
                    let _ = sock.flush().await;
                });
            }
        });

        // Loopback mock server: guard_egress off (as in dev/tests), force HTTP/1.1 over the raw socket.
        let client = build_client(ClientOpts { force_http1: true, guard_egress: false }).unwrap();
        let resp = client.get(format!("http://{addr}/start")).send().await.unwrap();
        assert_eq!(resp.status().as_u16(), 302, "client must surface the 3xx, not follow it");
        // Give any erroneous follow-up request a moment to land before asserting it never happened.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(leaked.load(Ordering::SeqCst), 0, "redirect target must never be requested");
    }
}
