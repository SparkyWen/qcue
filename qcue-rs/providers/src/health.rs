// QCue S1-R14 — cheap per-provider health check; never panics.
// SBX-R4 — SSRF guard: base_url is tenant-supplied; must be egress-guarded in prod (same surface as
// the dispatch client) — validate the host + build the client with the GuardedResolver.
use http::client::{build_client, opts_from_env, validate_base_url_security};

pub async fn health_check(base_url: &str) -> Result<bool, String> {
    let (opts, allow_insecure) = opts_from_env();
    validate_base_url_security(base_url, allow_insecure).map_err(|e| e.to_string())?;
    let client = build_client(opts).map_err(|e| e.to_string())?;
    match client
        .get(base_url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => Ok(resp.status().as_u16() < 500),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Unit-testable seam: validate_base_url_security is a pure fn; we assert SSRF blocking without
    // building a client or making a network call.
    #[test]
    fn health_check_blocks_ssrf_urls_in_prod() {
        for bad in [
            "https://169.254.169.254/",
            "https://10.0.0.5/",
            "http://192.168.1.10/",
            "https://metadata.google.internal/",
        ] {
            assert!(
                validate_base_url_security(bad, false).is_err(),
                "health_check must BLOCK {bad} in prod"
            );
        }
    }

    #[test]
    fn health_check_allows_public_urls_in_prod() {
        for good in ["https://api.openai.com/v1", "https://api.deepseek.com"] {
            assert!(
                validate_base_url_security(good, false).is_ok(),
                "health_check must ALLOW {good} in prod"
            );
        }
    }

    #[test]
    fn health_check_allows_loopback_in_dev() {
        assert!(validate_base_url_security("http://127.0.0.1:8080/", true).is_ok());
        assert!(validate_base_url_security("http://localhost:9200/", true).is_ok());
    }
}
