//! QCue S3-R11/R12/R13 — session JWT (HS256, iss/aud/exp) + the DUAL token resolver.
use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct JwtCfg {
    pub secret: Vec<u8>,
    pub iss: String,
    pub aud: String,
    pub ttl_secs: i64,
}

// QCue S3-R11 — claims. tenant_id is load-bearing for RLS; jti for revocation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionClaims {
    pub sub: Uuid,
    pub tenant_id: Uuid,
    pub role: String,
    pub jti: Uuid,
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
}
impl SessionClaims {
    pub fn new(sub: Uuid, tenant_id: Uuid, role: &str, jti: Uuid, c: &JwtCfg) -> Self {
        let now = Utc::now().timestamp();
        SessionClaims {
            sub,
            tenant_id,
            role: role.into(),
            jti,
            iss: c.iss.clone(),
            aud: c.aud.clone(),
            iat: now,
            exp: now + c.ttl_secs,
        }
    }
}

pub fn mint_session(claims: &SessionClaims, c: &JwtCfg) -> Result<String, jsonwebtoken::errors::Error> {
    encode(&Header::new(Algorithm::HS256), claims, &EncodingKey::from_secret(&c.secret))
}

pub fn verify_session(token: &str, c: &JwtCfg) -> Result<SessionClaims, jsonwebtoken::errors::Error> {
    let mut v = Validation::new(Algorithm::HS256);
    v.set_issuer(std::slice::from_ref(&c.iss));
    v.set_audience(std::slice::from_ref(&c.aud));
    v.validate_exp = true;
    Ok(decode::<SessionClaims>(token, &DecodingKey::from_secret(&c.secret), &v)?.claims)
}

#[derive(Debug, PartialEq)]
pub enum TokenSource {
    Header,
    Query,
}

/// DUAL extractor (S3-R12): Authorization Bearer FIRST, then ?token= — but ?token= only on SSE GET routes (S3-R13).
pub fn resolve_token(
    authz: Option<&str>,
    query_token: Option<&str>,
    is_sse_route: bool,
) -> Option<(TokenSource, String)> {
    if let Some(h) = authz
        && let Some(rest) = h.strip_prefix("Bearer ")
    {
        return Some((TokenSource::Header, rest.to_string()));
    }
    if is_sse_route
        && let Some(q) = query_token
    {
        return Some((TokenSource::Query, q.to_string()));
    }
    None
}
