//! QCue S3-R61..R65 — security headers, rate-limit (120/60s; 20/60s on /auth), body-cap 256KB, Origin-reject 403.
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::state::AppState;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use tower_http::set_header::SetResponseHeaderLayer;

/// JSON body cap (256 KB) applied to non-multipart routes (S3-R63).
pub const BODY_LIMIT_BYTES: usize = 256 * 1024;

const WINDOW: Duration = Duration::from_secs(60);
const GLOBAL_LIMIT: u32 = 120; // 120 / 60s
const AUTH_LIMIT: u32 = 20; //  20 / 60s on /v1/auth/*
/// Cap on distinct IP buckets held in memory. The limiter keys on client IP, so without a bound a long
/// uptime — or an attacker rotating source IPs — would grow the map indefinitely. At the cap we drop
/// every bucket whose window has fully elapsed (equivalent to absent), amortizing the sweep.
const MAX_TRACKED_KEYS: usize = 100_000;

/// Reject a WSS/SSE upgrade bearing a foreign Origin (anti-DNS-rebinding, S3-R65).
pub async fn origin_reject(State(st): State<AppState>, req: Request, next: Next) -> Result<Response, StatusCode> {
    if let Some(origin) = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let allowed = st.cfg.app_origins.iter().any(|o| o == origin);
        if !allowed {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    Ok(next.run(req).await)
}

/// Tag JSON responses with an explicit `; charset=utf-8`. axum's `Json` emits bare `application/json`,
/// which the Dart `http` client (and other latin-1-defaulting clients) decode as ISO-8859-1 — mojibake
/// for CJK text. Adding the charset makes EVERY client/tool decode UTF-8 correctly (the cross-client,
/// defense-in-depth fix). Non-JSON responses (SSE `text/event-stream`, octet-stream) are left untouched.
pub async fn json_charset(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let is_bare_json =
        res.headers().get(header::CONTENT_TYPE).map(HeaderValue::as_bytes) == Some(b"application/json");
    if is_bare_json {
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
    }
    res
}

pub fn security_headers() -> Vec<SetResponseHeaderLayer<HeaderValue>> {
    vec![
        SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        SetResponseHeaderLayer::overriding(
            header::HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ),
        SetResponseHeaderLayer::overriding(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ),
        SetResponseHeaderLayer::overriding(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")),
        SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
        ),
    ]
}

// ── rate limit (two-bucket, IP-keyed) ───────────────────────────────────────────────────────
struct Bucket {
    count: u32,
    window_start: Instant,
}
fn limiter() -> &'static Mutex<HashMap<String, Bucket>> {
    static L: OnceLock<Mutex<HashMap<String, Bucket>>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Extract the client IP, honoring `X-Forwarded-For` ONLY when the immediate peer is the trusted
/// proxy (S3-R64); otherwise the socket peer address. Never trusts a spoofable header blindly.
fn client_ip(req: &Request, trusted_proxy: &str) -> String {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "0.0.0.0".to_string());
    if !trusted_proxy.is_empty()
        && peer == trusted_proxy
        && let Some(xff) = req.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(first) = xff.split(',').next()
    {
        return first.trim().to_string();
    }
    peer
}

/// Hash an IP for storage (privacy — never persist a raw IP). Stable, non-cryptographic is fine here.
pub fn ip_hash(ip: &str) -> String {
    // FNV-1a over the bytes → hex; the auth audit writer persists this, never the raw IP.
    let mut h: u128 = 0xcbf29ce484222325;
    for b in ip.bytes() {
        h ^= b as u128;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:032x}")
}

/// IP-keyed rate limiter: 120/60s globally, 20/60s on `/v1/auth/*`. The over-limit request → 429
/// with `Retry-After` (S3-R61/R62). `/healthz` + `/readyz` are exempt (S3-R66).
pub async fn rate_limit(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    if path == "/healthz" || path == "/readyz" {
        return next.run(req).await;
    }
    let is_auth = path.starts_with("/v1/auth/");
    let limit = if is_auth { AUTH_LIMIT } else { GLOBAL_LIMIT };
    let ip = client_ip(&req, &st.cfg.trusted_proxy);
    let key = if is_auth { format!("auth:{ip}") } else { format!("glob:{ip}") };

    let over = bump_and_check(key, limit);

    if over {
        let mut resp = Response::new(axum::body::Body::from(
            r#"{"error":{"code":-32001,"message":"rate limited"}}"#,
        ));
        *resp.status_mut() = StatusCode::TOO_MANY_REQUESTS;
        resp.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        resp.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
        return resp;
    }
    next.run(req).await
}

/// Increment the bucket for `key` and report whether it now exceeds `limit`. The `MutexGuard` is
/// scoped to this sync fn so it is never held across an `.await` (keeps the middleware future Send).
fn bump_and_check(key: String, limit: u32) -> bool {
    let mut map = match limiter().lock() {
        Ok(m) => m,
        Err(_) => return false, // never fail closed on a poisoned lock
    };
    let now = Instant::now();
    // Reap expired buckets when the map grows large, so it can never leak memory without bound.
    if map.len() >= MAX_TRACKED_KEYS {
        map.retain(|_, b| now.duration_since(b.window_start) < WINDOW);
    }
    let bucket = map.entry(key).or_insert(Bucket { count: 0, window_start: now });
    if now.duration_since(bucket.window_start) >= WINDOW {
        bucket.count = 0;
        bucket.window_start = now;
    }
    bucket.count += 1;
    bucket.count > limit
}

/// Reset the rate-limit buckets (test isolation; the limiter is process-global).
pub fn reset_rate_limit() {
    if let Ok(mut m) = limiter().lock() {
        m.clear();
    }
}
