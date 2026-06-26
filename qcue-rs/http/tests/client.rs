// QCue S1-R15, S1-R16 — base_url security/versioning + HTTP1 escape hatch.
// The plan's literal `..Default::default()` on a single-field struct trips clippy::needless_update.
#![allow(clippy::needless_update)]
use http::client::{build_client, validate_base_url_security, versioned_base_url, ClientOpts};

#[test]
fn test_base_url_guard() {
    // In prod (allow_insecure=false): plain http is rejected for ALL hosts; https private IPs rejected too.
    assert!(validate_base_url_security("http://api.example.com", false).is_err());
    assert!(validate_base_url_security("http://127.0.0.1:8080", false).is_err()); // plain http rejected in prod
    assert!(validate_base_url_security("http://localhost:1234", false).is_err()); // plain http rejected in prod
    assert!(validate_base_url_security("https://api.openai.com", false).is_ok()); // tls ok
    assert!(validate_base_url_security("http://api.example.com", true).is_ok()); // override flag
    // In dev (allow_insecure=true): loopback http is fine for mock servers.
    assert!(validate_base_url_security("http://127.0.0.1:8080", true).is_ok());
    assert!(validate_base_url_security("http://localhost:1234", true).is_ok());
}

#[test]
fn test_versioned_base_url() {
    // DeepSeek uses /beta for beta features, but chat/completions must hit /v1.
    assert_eq!(
        versioned_base_url("https://api.deepseek.com/beta", "chat/completions"),
        "https://api.deepseek.com/v1/chat/completions"
    );
    // a bare host gets /v1 inserted.
    assert_eq!(
        versioned_base_url("https://api.openai.com", "chat/completions"),
        "https://api.openai.com/v1/chat/completions"
    );
    // an explicit /v1 is preserved.
    assert_eq!(
        versioned_base_url("https://api.openai.com/v1", "chat/completions"),
        "https://api.openai.com/v1/chat/completions"
    );
}

#[test]
fn test_force_http1_toggle() {
    // With force_http1, the built client is configured for HTTP/1.1 (no panic; builds).
    let c = build_client(ClientOpts { force_http1: true, ..Default::default() });
    assert!(c.is_ok());
    let c2 = build_client(ClientOpts { force_http1: false, ..Default::default() });
    assert!(c2.is_ok());
}
