//! QCue S3-R5/S3-R6 — TenantCtx + the FromRequestParts extractor that runs SET LOCAL app.tenant_id.
use crate::auth::jwt::{resolve_token, verify_session, JwtCfg};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub use app_server_protocol::Role;

pub type TenantTx = Transaction<'static, Postgres>;

/// Open a request-scoped tx on qcue_app and bind app.tenant_id for the whole request (S3-R5 steps 3-4).
pub async fn open_tenant_tx(pool: &PgPool, tenant_id: Uuid) -> Result<TenantTx, ApiError> {
    let mut tx = pool.begin().await?;
    // SET LOCAL via set_config(..., is_local=true): tx-scoped (S3-R2), rolled back at tx end.
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// The request-scoped tenant context every handler receives (S3-R5).
pub struct TenantCtx {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub role: Role,
    pub jti: Uuid,
    pub device_id: Option<Uuid>,
    pub tx: TenantTx, // already has SET LOCAL applied
    /// The raw request query string (so SSE handlers can read `?q=`/`?since_seq=` without a second
    /// extractor). `None` when the URL had no query.
    pub query: Option<String>,
}

impl TenantCtx {
    /// Read a URL query parameter by key (e.g. the recall `?q=` question or `?since_seq=`).
    pub fn query_param(&self, key: &str) -> Option<String> {
        self.query.as_deref().and_then(|q| url_query_value(q, key))
    }
}

// The SSE GET allowlist (S3-R13): only these paths accept ?token=.
const SSE_ROUTES: &[&str] = &[
    "/v1/ingest/",
    "/v1/recall/",
    "/v1/wiki/query/",
    "/v1/dream/",
    "/v1/sync/pull",
    // the WSS turn channel upgrade is a GET a browser socket can't add a header to → allow ?token=.
    "/v1/thread/",
];
fn is_sse_route(path: &str, method: &axum::http::Method) -> bool {
    method == axum::http::Method::GET && SSE_ROUTES.iter().any(|p| path.starts_with(p))
}

fn url_query_value(q: &str, key: &str) -> Option<String> {
    q.split('&').find_map(|kv| {
        let mut it = kv.splitn(2, '=');
        match (it.next(), it.next()) {
            (Some(k), Some(v)) if k == key => Some(percent_decode(v)),
            _ => None,
        }
    })
}

/// Minimal percent-decoding for query values (`%20`→space, `%E4%BD%A0`→你, …). The recall `?q=`
/// question MUST be decoded before retrieval — otherwise the model/FTS see `%20` garbage and keyword
/// search is wrecked. Leaves `+` untouched (the Dart/JS clients encode spaces as `%20`, not `+`) and
/// passes any malformed `%` sequence through unchanged. JWT/base64url tokens contain no `%`, so this
/// is a no-op for the `?token=` value.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) =
                ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl FromRequestParts<AppState> for TenantCtx {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, st: &AppState) -> Result<Self, Self::Rejection> {
        let authz = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        let qtoken = parts.uri.query().and_then(|q| url_query_value(q, "token"));
        let sse = is_sse_route(parts.uri.path(), &parts.method);
        let (_src, token) = resolve_token(authz, qtoken.as_deref(), sse).ok_or(ApiError::Unauthorized)?;
        let jwtcfg = JwtCfg {
            secret: st.cfg.jwt_secret.clone(),
            iss: "qcue".into(),
            aud: "qcue-app".into(),
            ttl_secs: st.cfg.access_ttl_secs,
        };
        let claims = verify_session(&token, &jwtcfg).map_err(|_| ApiError::Unauthorized)?;
        // S3-R5 step 2: reject revoked/expired jti BEFORE any tenant query. sessions has FORCE RLS,
        // so the revocation read runs inside a GUC-bound tx on the narrow auth_pool.
        let mut auth_tx = st.auth_pool.begin().await.map_err(ApiError::Db)?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(claims.tenant_id.to_string())
            .execute(&mut *auth_tx)
            .await
            .map_err(ApiError::Db)?;
        let live: Option<(Uuid,)> = sqlx::query_as(
            "SELECT jti FROM sessions WHERE tenant_id=$1 AND jti=$2 AND revoked_at IS NULL AND expires_at > now()",
        )
        .bind(claims.tenant_id)
        .bind(claims.jti)
        .fetch_optional(&mut *auth_tx)
        .await
        .map_err(ApiError::Db)?;
        auth_tx.commit().await.map_err(ApiError::Db)?;
        if live.is_none() {
            return Err(ApiError::Unauthorized);
        }
        let tx = open_tenant_tx(&st.pool, claims.tenant_id).await?;
        Ok(TenantCtx {
            tenant_id: claims.tenant_id,
            user_id: claims.sub,
            role: Role::Owner,
            jti: claims.jti,
            device_id: None,
            tx,
            query: parts.uri.query().map(str::to_string),
        })
    }
}

#[cfg(test)]
mod query_tests {
    use super::url_query_value;
    #[test]
    fn percent_decodes_the_recall_question() {
        assert_eq!(
            url_query_value("q=What%20backend%3F&token=abc", "q").as_deref(),
            Some("What backend?")
        );
    }
    #[test]
    fn decodes_utf8_cjk() {
        assert_eq!(url_query_value("q=%E4%BD%A0%E5%A5%BD", "q").as_deref(), Some("你好"));
    }
    #[test]
    fn token_with_no_percent_is_unchanged() {
        // JWT base64url (dots, -, _) must round-trip untouched.
        assert_eq!(
            url_query_value("token=eyJhbGc.aB-_123.Xy", "token").as_deref(),
            Some("eyJhbGc.aB-_123.Xy")
        );
    }
    #[test]
    fn malformed_percent_passes_through() {
        assert_eq!(url_query_value("q=50%off", "q").as_deref(), Some("50%off"));
    }
}
