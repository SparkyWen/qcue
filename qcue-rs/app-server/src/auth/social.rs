//! QCue S3-R7 / NG-R1..R6 — Apple/Google id_token verify against the provider JWKS (cached, TTL) +
//! iss/aud/exp/email_verified. The JWKS is fetched live from Google and cached by `kid`.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::Mutex;

const GOOGLE_CERTS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const APPLE_CERTS_URL: &str = "https://appleid.apple.com/auth/keys";
const JWKS_FALLBACK_TTL: Duration = Duration::from_secs(3600);

/// The JWKS endpoint for a social provider. Apple's keys are the same RSA JWK shape as Google's, so the
/// only per-provider difference is the URL.
fn certs_url(provider: &str) -> Option<&'static str> {
    match provider {
        "google" => Some(GOOGLE_CERTS_URL),
        "apple" => Some(APPLE_CERTS_URL),
        _ => None,
    }
}

#[derive(Clone)]
pub struct SocialCfg {
    pub google_auds: Vec<String>,
    pub apple_auds: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct IdTokenClaims {
    pub sub: String,
    pub email: Option<String>,
    pub iss: String,
    pub aud: String,
    pub exp: i64,
    // Apple sometimes encodes this as the STRING "true"/"false" rather than a JSON bool; the lenient
    // deserializer accepts either so the whole token still decodes. (The Apple path doesn't check it, but
    // the field must still parse or the entire `decode` fails.) Google sends a real bool.
    #[serde(default, deserialize_with = "de_bool_lenient")]
    pub email_verified: Option<bool>,
    #[serde(default)]
    pub nonce: Option<String>,
}

/// Accept `email_verified` as either a JSON bool or the string `"true"`/`"false"` (Apple emits the string
/// form in some identity tokens; a strict `Option<bool>` would fail the whole decode).
fn de_bool_lenient<'de, D>(d: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolOrStr {
        B(bool),
        S(String),
    }
    Ok(match Option::<BoolOrStr>::deserialize(d)? {
        Some(BoolOrStr::B(b)) => Some(b),
        Some(BoolOrStr::S(s)) => Some(s == "true"),
        None => None,
    })
}

/// Verify an IdP id_token. Returns the subject + email on success; rejects bad aud/iss/exp/sig and
/// (for Google) an unverified email (S3-R7 / NG-R4..R6).
pub async fn verify_id_token(
    provider: &str,
    id_token: &str,
    cfg: &SocialCfg,
    jwks: &Jwks,
) -> Result<IdTokenClaims, ()> {
    let header = decode_header(id_token).map_err(|_| ())?;
    let kid = header.kid.ok_or(())?;
    // Google issues either `accounts.google.com` or `https://accounts.google.com` (NG-R4).
    let (issuers, auds): (Vec<&str>, &[String]) = match provider {
        "google" => (vec!["https://accounts.google.com", "accounts.google.com"], &cfg.google_auds),
        "apple" => (vec!["https://appleid.apple.com"], &cfg.apple_auds),
        _ => return Err(()),
    };
    if auds.is_empty() {
        return Err(());
    }
    let key = jwks.key_for(provider, &kid).await.ok_or(())?;
    let mut v = Validation::new(Algorithm::RS256);
    v.set_issuer(&issuers);
    v.set_audience(auds);
    v.validate_exp = true;
    let data = decode::<IdTokenClaims>(id_token, &key, &v).map_err(|_| ())?;
    // The returned email is used downstream as an account-LINKING key (link_or_create_user), so it must be
    // proven by the IdP. Require email_verified for BOTH providers: Google (NG-R6) AND Apple — a managed
    // "Sign in with Apple at Work & School" id can carry an org-set, domain-UNverified email, so accepting
    // it unconditionally would let such a token link/adopt another user's account. Consumer Apple IDs and
    // the private-relay address always report verified, so this rejects only the unsafe managed-unverified
    // case. (When the token carries no email at all, the caller already refuses to link.)
    if data.claims.email.is_some() && data.claims.email_verified != Some(true) {
        return Err(());
    }
    Ok(data.claims)
}

/// Parse a Google JWKS document (`{"keys":[{kid,n,e,...}]}`) into kid → RSA DecodingKey (NG-R1).
pub fn parse_google_jwks(v: &serde_json::Value) -> HashMap<String, DecodingKey> {
    let mut out = HashMap::new();
    let Some(keys) = v.get("keys").and_then(|k| k.as_array()) else { return out };
    for k in keys {
        let (Some(kid), Some(n), Some(e)) = (
            k.get("kid").and_then(|x| x.as_str()),
            k.get("n").and_then(|x| x.as_str()),
            k.get("e").and_then(|x| x.as_str()),
        ) else { continue };
        if let Ok(dk) = DecodingKey::from_rsa_components(n, e) {
            out.insert(kid.to_string(), dk);
        }
    }
    out
}

struct JwksCache {
    keys: HashMap<String, DecodingKey>,
    expires_at: Instant,
}

/// Per-provider JWKS cache for the social-login signing certs (Google + Apple), TTL-refreshed live from
/// each provider's certs endpoint ([`certs_url`]). Keyed by provider so an Apple `kid` is looked up
/// against Apple's key set, never Google's.
pub struct Jwks {
    caches: Mutex<HashMap<String, JwksCache>>,
    /// Single-flight guard: coalesces concurrent cold/stale refetches so a sign-in burst does not
    /// fan out N identical JWKS fetches — one task fetches, the rest re-check the cache.
    refresh_lock: Mutex<()>,
}

impl Default for Jwks {
    fn default() -> Self {
        Jwks { caches: Mutex::new(HashMap::new()), refresh_lock: Mutex::new(()) }
    }
}

impl Jwks {
    pub fn new() -> Self {
        Jwks::default()
    }

    /// Test seam: a JWKS pre-seeded with one key under BOTH providers, never expiring — no network
    /// (NG-R3). Seeding both lets the Google and Apple verify tests share one injected key.
    pub fn with_test_key(kid: &str, key: DecodingKey) -> Self {
        let mut keys = HashMap::new();
        keys.insert(kid.to_string(), key);
        let exp = Instant::now() + Duration::from_secs(86_400);
        let mut caches = HashMap::new();
        caches.insert("google".to_string(), JwksCache { keys: keys.clone(), expires_at: exp });
        caches.insert("apple".to_string(), JwksCache { keys, expires_at: exp });
        Jwks { caches: Mutex::new(caches), refresh_lock: Mutex::new(()) }
    }

    /// A fresh-cache read: returns the key for `(provider, kid)` iff that provider's cache is unexpired
    /// and holds it.
    async fn cached_fresh(&self, provider: &str, kid: &str) -> Option<DecodingKey> {
        let guard = self.caches.lock().await;
        if let Some(c) = guard.get(provider)
            && c.expires_at > Instant::now()
            && let Some(k) = c.keys.get(kid)
        {
            return Some(k.clone());
        }
        None
    }

    /// Return the DecodingKey for `(provider, kid)`, fetching+caching that provider's JWKS on a
    /// cold/stale cache. Concurrent cold/stale callers are coalesced by `refresh_lock`: the first
    /// fetches, the rest wait and re-read the freshly-populated cache (NG-R1, single-flight).
    pub async fn key_for(&self, provider: &str, kid: &str) -> Option<DecodingKey> {
        if let Some(k) = self.cached_fresh(provider, kid).await {
            return Some(k);
        }
        // Serialize refetches; re-check the cache in case a peer just refreshed it while we waited.
        let _refresh = self.refresh_lock.lock().await;
        if let Some(k) = self.cached_fresh(provider, kid).await {
            return Some(k);
        }
        // Cold or stale (or unknown kid → the provider may have rotated): refetch once.
        let url = certs_url(provider)?;
        let resp = reqwest::Client::new()
            .get(url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .ok()?;
        let ttl = resp
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|h| h.to_str().ok())
            .and_then(parse_max_age)
            .unwrap_or(JWKS_FALLBACK_TTL);
        let body: serde_json::Value = resp.json().await.ok()?;
        let keys = parse_google_jwks(&body);
        let found = keys.get(kid).cloned();
        let mut guard = self.caches.lock().await;
        guard.insert(provider.to_string(), JwksCache { keys, expires_at: Instant::now() + ttl });
        found
    }
}

/// Extract `max-age=<secs>` from a Cache-Control header value.
fn parse_max_age(h: &str) -> Option<Duration> {
    for part in h.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("max-age=")
            && let Ok(secs) = rest.parse::<u64>()
        {
            return Some(Duration::from_secs(secs));
        }
    }
    None
}
