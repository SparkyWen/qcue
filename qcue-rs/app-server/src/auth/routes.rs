//! QCue S3-R7/R9/R15 — auth routes on the narrow auth_pool. Sessions rows + audit. Magic-link single-use.
//!
//! Magic-link tokens live in [`crate::auth::magic`]: Redis-backed (cross-worker, GETDEL one-shot
//! consume) when `REDIS_URL` is set, else a process-local in-memory map. The contract the tests pin
//! (unknown email → 200 + no token; verify works exactly once; a second verify → 401) holds either way.
use std::time::Duration;

use crate::auth::audit::audit;
use crate::auth::magic;
use crate::auth::jwt::{mint_session, verify_session, JwtCfg, SessionClaims};
use crate::auth::password::{hash_password, needs_rehash, verify_password, VerifyOutcome};
use crate::error::ApiError;
use crate::state::AppState;
use axum::{extract::State, routing::post, Json, Router};
use chrono::{Duration as ChronoDuration, Utc};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/signup", post(signup))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/refresh", post(refresh))
        .route("/v1/auth/logout", post(logout))
        .route("/v1/auth/magic/request", post(magic_request))
        .route("/v1/auth/magic/verify", post(magic_verify))
        .route("/v1/auth/social", post(social))
        .route("/v1/auth/oidc", post(oidc))
}

fn access_cfg(st: &AppState) -> JwtCfg {
    JwtCfg { secret: st.cfg.jwt_secret.clone(), iss: "qcue".into(), aud: "qcue-app".into(), ttl_secs: st.cfg.access_ttl_secs }
}
fn refresh_cfg(st: &AppState) -> JwtCfg {
    JwtCfg { secret: st.cfg.jwt_secret.clone(), iss: "qcue".into(), aud: "qcue-refresh".into(), ttl_secs: 60 * 60 * 24 * 30 }
}

/// Issue a single-use magic token for a KNOWN email (returns None for unknown email — no enumeration).
/// Exposed for tests + reused by `magic_request`. Persisted in the (Redis-or-memory) magic store.
pub async fn test_issue_magic(auth_pool: &sqlx::PgPool, email: &str) -> Option<String> {
    let row = sqlx::query("SELECT id, tenant_id FROM users WHERE email=$1")
        .bind(email)
        .fetch_optional(auth_pool)
        .await
        .ok()??;
    let user_id: Uuid = row.get("id");
    let tenant_id: Uuid = row.get("tenant_id");
    let token = new_magic_token();
    magic::store().await.put(&token, tenant_id, user_id, Duration::from_secs(15 * 60)).await;
    Some(token)
}

/// S3-R9 — mint a single-use magic-link token: a dedicated 256-bit CSPRNG secret (two v4 UUIDs =
/// 244 random bits, getrandom/OsRng-backed) with NO embedded timestamp. Earlier this was a UUIDv7,
/// which leaks its 48-bit issuance time and spends entropy on it; a login token should be pure random.
fn new_magic_token() -> String {
    format!("mgc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Consume a magic token exactly once (single-use, S3-R9). Returns (tenant, user) or None.
async fn consume_magic(token: &str) -> Option<(Uuid, Uuid)> {
    magic::store().await.consume(token).await
}

/// Insert a live sessions row for a freshly minted jti (sessions has FORCE RLS → bind the GUC in-tx).
async fn insert_session(
    st: &AppState,
    tenant: Uuid,
    user: Uuid,
    jti: Uuid,
    expires: chrono::DateTime<Utc>,
) -> Result<(), ApiError> {
    let mut tx = st.auth_pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO sessions(tenant_id,user_id,jti,expires_at) VALUES ($1,$2,$3,$4)")
        .bind(tenant)
        .bind(user)
        .bind(jti)
        .bind(expires)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Revoke every live session for a user (used on refresh-rotation + logout-all + account deletion).
pub(crate) async fn revoke_user_sessions(st: &AppState, tenant: Uuid, user: Uuid) -> Result<(), ApiError> {
    let mut tx = st.auth_pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE sessions SET revoked_at=now() WHERE tenant_id=$1 AND user_id=$2 AND revoked_at IS NULL")
        .bind(tenant)
        .bind(user)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Revoke EXACTLY ONE live session by its jti (used on refresh-rotation, AUTH-R1). Unlike
/// `revoke_user_sessions` this leaves the user's other live sessions (other devices) untouched.
async fn revoke_session(st: &AppState, tenant: Uuid, jti: Uuid) -> Result<(), ApiError> {
    let mut tx = st.auth_pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE sessions SET revoked_at=now() WHERE tenant_id=$1 AND jti=$2 AND revoked_at IS NULL")
        .bind(tenant)
        .bind(jti)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn issue_pair(st: &AppState, tenant: Uuid, user: Uuid, role: &str) -> Result<serde_json::Value, ApiError> {
    let access_jti = Uuid::now_v7();
    let access_claims = SessionClaims::new(user, tenant, role, access_jti, &access_cfg(st));
    let access = mint_session(&access_claims, &access_cfg(st)).map_err(|_| ApiError::Unauthorized)?;
    let access_exp = Utc::now() + ChronoDuration::seconds(st.cfg.access_ttl_secs);
    insert_session(st, tenant, user, access_jti, access_exp).await?;

    let refresh_jti = Uuid::now_v7();
    let refresh_claims = SessionClaims::new(user, tenant, role, refresh_jti, &refresh_cfg(st));
    let refresh = mint_session(&refresh_claims, &refresh_cfg(st)).map_err(|_| ApiError::Unauthorized)?;
    let refresh_exp = Utc::now() + ChronoDuration::days(30);
    insert_session(st, tenant, user, refresh_jti, refresh_exp).await?;

    Ok(serde_json::json!({
        "access_jwt": access,
        "refresh_jwt": refresh,
        "expires_at": access_exp,
    }))
}

// ── routes ────────────────────────────────────────────────────────────────────────────────
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginReq {
    email: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignupReq {
    email: String,
    password: String,
}

/// `POST /v1/auth/signup` — minimal email/password onboarding for multi-user cloud sync. Creates one
/// tenant per signup (D8 solo accounts) + the owner `users` row (argon2id hash, reusing the login
/// hashing), then returns the SAME `{access_jwt, refresh_jwt, expires_at}` shape as login. A duplicate
/// email (the global UNIQUE) fails closed as a conflict rather than leaking which emails exist.
async fn signup(State(st): State<AppState>, Json(req): Json<SignupReq>) -> Result<Json<serde_json::Value>, ApiError> {
    // Reject obviously-empty inputs early (the password policy is otherwise the hasher's concern).
    if req.email.trim().is_empty() || req.password.is_empty() {
        return Err(ApiError::Unauthorized);
    }
    // Reject a duplicate email up front (global UNIQUE on users.email; B-R6 exception).
    let existing = sqlx::query("SELECT 1 AS x FROM users WHERE email = $1")
        .bind(&req.email)
        .fetch_optional(&st.auth_pool)
        .await?;
    if existing.is_some() {
        // No dedicated 409 variant; a 400 with a stable message keeps the client logic simple while
        // not leaking timing/enumeration beyond the (already global-UNIQUE) email constraint.
        return Err(ApiError::BadRequest("email already registered".into()));
    }
    let hash = hash_password(&req.password).map_err(|_| ApiError::Unauthorized)?;

    let tenant_id = Uuid::now_v7();
    let user_id = Uuid::now_v7();
    let mut tx = st.auth_pool.begin().await?;
    // `tenants` is the global root (no RLS) — seed it first to satisfy the users FK. namespace is the
    // immutable object-store/vault key `t/<id>` (Appendix B §4.1 / §9).
    sqlx::query(
        "INSERT INTO tenants (id, slug, display_name, namespace) VALUES ($1, $2, $2, $3)",
    )
    .bind(tenant_id)
    .bind(tenant_id.to_string())
    .bind(format!("t/{tenant_id}"))
    .execute(&mut *tx)
    .await?;
    // `users` is FORCE RLS → bind the GUC in-tx before the INSERT (WITH CHECK tenant_id = app_tenant()).
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO users (id, tenant_id, email, password_hash, role) VALUES ($1, $2, $3, $4, 'owner')",
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind(&req.email)
    .bind(&hash)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let body = issue_pair(&st, tenant_id, user_id, "owner").await?;
    audit(&st.auth_pool, Some(tenant_id), Some(user_id), "auth.signup.ok", None, serde_json::json!({})).await;
    Ok(Json(body))
}

async fn login(State(st): State<AppState>, Json(req): Json<LoginReq>) -> Result<Json<serde_json::Value>, ApiError> {
    // bootstrap email lookup on the narrow auth_pool (S3-R4).
    let row = sqlx::query("SELECT id, tenant_id, password_hash, role, is_active FROM users WHERE email=$1")
        .bind(&req.email)
        .fetch_optional(&st.auth_pool)
        .await?;
    let (uid, tid, hash, role, active) = match &row {
        Some(r) => (
            Some(r.get::<Uuid, _>("id")),
            Some(r.get::<Uuid, _>("tenant_id")),
            r.get::<Option<String>, _>("password_hash"),
            r.get::<String, _>("role"),
            r.get::<bool, _>("is_active"),
        ),
        None => (None, None, None, "owner".into(), false),
    };
    // constant-time: verify_password(None,...) hits DUMMY_HASH on unknown email (S3-R8).
    let outcome = verify_password(hash.as_deref(), &req.password);
    if outcome != VerifyOutcome::Match || !active {
        audit(&st.auth_pool, tid, uid, "auth.login.failed", None, serde_json::json!({"email": req.email})).await;
        return Err(ApiError::Unauthorized);
    }
    let (uid, tid) = match (uid, tid) {
        (Some(u), Some(t)) => (u, t),
        _ => return Err(ApiError::Unauthorized),
    };
    // lazy rehash in the same flow (S3-R10). The UPDATE touches the RLS-forced `users` row, so it
    // runs in a GUC-bound tx (the tenant is now known from the bootstrap lookup).
    if let Some(h) = &hash
        && needs_rehash(h)
        && let Ok(nh) = hash_password(&req.password)
        && let Ok(mut tx) = st.auth_pool.begin().await
    {
        let _ = sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tid.to_string())
            .execute(&mut *tx)
            .await;
        let _ = sqlx::query("UPDATE users SET password_hash=$1 WHERE id=$2")
            .bind(nh)
            .bind(uid)
            .execute(&mut *tx)
            .await;
        let _ = tx.commit().await;
    }
    let body = issue_pair(&st, tid, uid, &role).await?;
    audit(&st.auth_pool, Some(tid), Some(uid), "auth.login.ok", None, serde_json::json!({})).await;
    Ok(Json(body))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RefreshReq {
    refresh_token: String,
}

async fn refresh(State(st): State<AppState>, Json(req): Json<RefreshReq>) -> Result<Json<serde_json::Value>, ApiError> {
    let claims = verify_session(&req.refresh_token, &refresh_cfg(&st)).map_err(|_| ApiError::Unauthorized)?;
    // the refresh jti must still be live (not revoked / not expired).
    let live = session_is_live(&st, claims.tenant_id, claims.jti).await?;
    if !live {
        return Err(ApiError::Unauthorized);
    }
    // rotate: revoke ONLY the presented refresh jti (one-time-use per chain, S3-R15), then issue a
    // fresh pair. Sibling sessions (other devices) stay live — kills the cascade (AUTH-R1/AUTH-D2).
    revoke_session(&st, claims.tenant_id, claims.jti).await?;
    let body = issue_pair(&st, claims.tenant_id, claims.sub, &claims.role).await?;
    audit(&st.auth_pool, Some(claims.tenant_id), Some(claims.sub), "auth.refresh.ok", None, serde_json::json!({})).await;
    Ok(Json(body))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogoutReq {
    refresh_token: String,
}

async fn logout(State(st): State<AppState>, Json(req): Json<LogoutReq>) -> Result<Json<serde_json::Value>, ApiError> {
    // logout is idempotent + best-effort: revoke the bearer's sessions if the token still parses.
    if let Ok(claims) = verify_session(&req.refresh_token, &refresh_cfg(&st)) {
        revoke_user_sessions(&st, claims.tenant_id, claims.sub).await?;
        audit(&st.auth_pool, Some(claims.tenant_id), Some(claims.sub), "auth.logout.ok", None, serde_json::json!({})).await;
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MagicRequestReq {
    email: String,
}

async fn magic_request(State(st): State<AppState>, Json(req): Json<MagicRequestReq>) -> Result<Json<serde_json::Value>, ApiError> {
    // unknown email → 200 + no token row (no account enumeration, S3-R9).
    let _ = test_issue_magic(&st.auth_pool, &req.email).await; // "email" the token in production
    audit(&st.auth_pool, None, None, "auth.magic.request", None, serde_json::json!({"email": req.email})).await;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MagicVerifyReq {
    token: String,
}

async fn magic_verify(State(st): State<AppState>, Json(req): Json<MagicVerifyReq>) -> Result<Json<serde_json::Value>, ApiError> {
    // single-use: consume removes the token, so a second verify → 401 (S3-R9).
    let (tenant, user) = consume_magic(&req.token).await.ok_or(ApiError::Unauthorized)?;
    let body = issue_pair(&st, tenant, user, "owner").await?;
    audit(&st.auth_pool, Some(tenant), Some(user), "auth.magic.verify.ok", None, serde_json::json!({})).await;
    Ok(Json(body))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SocialReq {
    provider: String,
    id_token: String,
}

async fn social(State(st): State<AppState>, Json(req): Json<SocialReq>) -> Result<Json<serde_json::Value>, ApiError> {
    use crate::auth::social::{verify_id_token, SocialCfg};
    let cfg = SocialCfg {
        google_auds: st.cfg.google_oauth_audiences.clone(),
        apple_auds: st.cfg.apple_oauth_audiences.clone(),
    };
    let claims = verify_id_token(&req.provider, &req.id_token, &cfg, &st.jwks)
        .await
        .map_err(|_| ApiError::Unauthorized)?;
    let email = match claims.email.as_deref() {
        Some(e) if !e.is_empty() => e.to_owned(),
        _ => return Err(ApiError::BadRequest("the signed-in identity has no email; cannot link a qcue account".into())),
    };
    let (uid, tid, role) = link_or_create_user(&st, &req.provider, &claims.sub, &email).await?;
    let body = issue_pair(&st, tid, uid, &role).await?;
    audit(&st.auth_pool, Some(tid), Some(uid), "auth.social.ok", None, serde_json::json!({"email": email, "provider": req.provider})).await;
    Ok(Json(body))
}

/// Find-or-create the qcue user for a verified social identity (NG-R8). Keys on
/// `oauth_identities(provider, subject)`; falls back to `users.email` to LINK an account that already
/// exists (email/password or a prior OIDC login), else creates tenant+user+oauth_identity. All writes
/// bind the tenant GUC in-tx so RLS WITH CHECK passes (mirrors `oidc`).
async fn link_or_create_user(
    st: &AppState,
    provider: &str,
    subject: &str,
    email: &str,
) -> Result<(Uuid, Uuid, String), ApiError> {
    // 1) Known identity? (bootstrap read: app_tenant() IS NULL widens SELECT on oauth_identities)
    if let Some(r) = sqlx::query(
        "SELECT u.id, u.tenant_id, u.role, u.is_active \
         FROM oauth_identities oi JOIN users u ON u.id = oi.user_id \
         WHERE oi.provider=$1 AND oi.subject=$2")
        .bind(provider)
        .bind(subject)
        .fetch_optional(&st.auth_pool)
        .await?
    {
        if !r.get::<bool, _>("is_active") {
            return Err(ApiError::Unauthorized);
        }
        return Ok((r.get::<Uuid, _>("id"), r.get::<Uuid, _>("tenant_id"), r.get::<String, _>("role")));
    }

    // 2) Existing user by email? → LINK this verified social identity to it. The IdP has just PROVEN the
    //    caller owns `email`. But the matched row may be an email/password account whose email was NEVER
    //    verified on the QCue side (signup does no confirmation, AUTH) — i.e. possibly an account
    //    pre-registered by an attacker who still knows the password (the classic→federated account
    //    pre-hijacking attack). So when the matched account has a password set, treat that password as
    //    untrusted: null it AND revoke its live sessions, so only the now-IdP-verified owner keeps access.
    if let Some(r) =
        sqlx::query("SELECT id, tenant_id, role, is_active, password_hash FROM users WHERE email=$1")
            .bind(email)
            .fetch_optional(&st.auth_pool)
            .await?
    {
        if !r.get::<bool, _>("is_active") {
            return Err(ApiError::Unauthorized);
        }
        let (uid, tid, role) = (r.get::<Uuid, _>("id"), r.get::<Uuid, _>("tenant_id"), r.get::<String, _>("role"));
        let had_password = r.get::<Option<String>, _>("password_hash").is_some();
        let mut tx = st.auth_pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)").bind(tid.to_string()).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO oauth_identities (tenant_id, user_id, provider, subject, email) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (provider, subject) DO NOTHING")
            .bind(tid).bind(uid).bind(provider).bind(subject).bind(email)
            .execute(&mut *tx).await?;
        if had_password {
            // Drop the unverified-provenance password so a pre-registering attacker can no longer log in.
            sqlx::query("UPDATE users SET password_hash=NULL WHERE id=$1").bind(uid).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        if had_password {
            // Revoke any pre-existing sessions (the new session minted by the caller, after this returns,
            // is unaffected). Lock out an attacker who pre-empted the account.
            revoke_user_sessions(st, tid, uid).await?;
            audit(&st.auth_pool, Some(tid), Some(uid), "auth.social.password_revoked_on_link", None, serde_json::json!({"email": email, "provider": provider})).await;
        }
        audit(&st.auth_pool, Some(tid), Some(uid), "auth.social.link", None, serde_json::json!({"email": email, "provider": provider})).await;
        return Ok((uid, tid, role));
    }

    // 3) Brand-new: create tenant + user (password_hash NULL, D11) + oauth_identity.
    let tenant_id = Uuid::now_v7();
    let user_id = Uuid::now_v7();
    let mut tx = st.auth_pool.begin().await?;
    sqlx::query("INSERT INTO tenants (id, slug, display_name, namespace) VALUES ($1,$2,$2,$3)")
        .bind(tenant_id).bind(tenant_id.to_string()).bind(format!("t/{tenant_id}"))
        .execute(&mut *tx).await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)").bind(tenant_id.to_string()).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO users (id, tenant_id, email, password_hash, role) VALUES ($1,$2,$3,NULL,'owner')")
        .bind(user_id).bind(tenant_id).bind(email)
        .execute(&mut *tx).await?;
    sqlx::query("INSERT INTO oauth_identities (tenant_id, user_id, provider, subject, email) VALUES ($1,$2,$3,$4,$5)")
        .bind(tenant_id).bind(user_id).bind(provider).bind(subject).bind(email)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    audit(&st.auth_pool, Some(tenant_id), Some(user_id), "auth.social.signup", None, serde_json::json!({"email": email, "provider": provider})).await;
    Ok((user_id, tenant_id, "owner".to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OidcReq {
    /// An access_token from the auth.qcue.cn OIDC provider (e.g. obtained via "Sign in with Google").
    access_token: String,
}

/// `POST /v1/auth/oidc` — bridge an auth.qcue.cn identity into a qcue session. The token is verified by the
/// provider's own `/oidc/v1/userinfo` (signature/exp/binding), which returns the email; we then find-or-create
/// the qcue user by that email and issue the normal `{access_jwt, refresh_jwt}`. This is how "Sign in with
/// Google" (federated through auth.qcue.cn) logs the user into qcue's own data. [Google login bridge]
async fn oidc(State(st): State<AppState>, Json(req): Json<OidcReq>) -> Result<Json<serde_json::Value>, ApiError> {
    // 1) Verify + read identity via the provider's userinfo (it does the JWKS signature + exp + binding checks).
    let userinfo_url = std::env::var("QCUE_OIDC_USERINFO_URL").unwrap_or_else(|_| "http://127.0.0.1:9210/oidc/v1/userinfo".into());
    let resp = reqwest::Client::new()
        .get(&userinfo_url)
        .header("host", "auth.qcue.cn")
        .bearer_auth(&req.access_token)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|_| ApiError::Unauthorized)?;
    if !resp.status().is_success() {
        return Err(ApiError::Unauthorized);
    }
    let info: serde_json::Value = resp.json().await.map_err(|_| ApiError::Unauthorized)?;
    let email = match info.get("email").and_then(|v| v.as_str()) {
        Some(e) if !e.is_empty() => e.to_owned(),
        _ => return Err(ApiError::BadRequest("the signed-in identity has no email; cannot link a qcue account".into())),
    };

    // 2) Find-or-create the qcue user by email (mirrors signup; OIDC users have no password). As in the
    //    social path, the provider has now PROVEN email ownership, so if the matched account carries an
    //    unverified-provenance password, null it + revoke its sessions to prevent account pre-hijacking.
    let existing =
        sqlx::query("SELECT id, tenant_id, role, is_active, password_hash FROM users WHERE email=$1")
            .bind(&email)
            .fetch_optional(&st.auth_pool)
            .await?;
    let (uid, tid, role) = if let Some(r) = &existing {
        if !r.get::<bool, _>("is_active") {
            return Err(ApiError::Unauthorized);
        }
        let (uid, tid, role) = (r.get::<Uuid, _>("id"), r.get::<Uuid, _>("tenant_id"), r.get::<String, _>("role"));
        if r.get::<Option<String>, _>("password_hash").is_some() {
            let mut tx = st.auth_pool.begin().await?;
            sqlx::query("SELECT set_config('app.tenant_id', $1, true)").bind(tid.to_string()).execute(&mut *tx).await?;
            sqlx::query("UPDATE users SET password_hash=NULL WHERE id=$1").bind(uid).execute(&mut *tx).await?;
            tx.commit().await?;
            revoke_user_sessions(&st, tid, uid).await?;
            audit(&st.auth_pool, Some(tid), Some(uid), "auth.oidc.password_revoked_on_link", None, serde_json::json!({"email": email})).await;
        }
        (uid, tid, role)
    } else {
        let tenant_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();
        let mut tx = st.auth_pool.begin().await?;
        sqlx::query("INSERT INTO tenants (id, slug, display_name, namespace) VALUES ($1,$2,$2,$3)")
            .bind(tenant_id)
            .bind(tenant_id.to_string())
            .bind(format!("t/{tenant_id}"))
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)").bind(tenant_id.to_string()).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO users (id, tenant_id, email, password_hash, role) VALUES ($1,$2,$3,NULL,'owner')")
            .bind(user_id)
            .bind(tenant_id)
            .bind(&email)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        audit(&st.auth_pool, Some(tenant_id), Some(user_id), "auth.oidc.signup", None, serde_json::json!({"email": email})).await;
        (user_id, tenant_id, "owner".to_string())
    };

    let body = issue_pair(&st, tid, uid, &role).await?;
    audit(&st.auth_pool, Some(tid), Some(uid), "auth.oidc.ok", None, serde_json::json!({"email": email})).await;
    Ok(Json(body))
}

/// Is a (tenant, jti) session live? (revocation gate read; sessions has FORCE RLS → bind GUC in-tx).
async fn session_is_live(st: &AppState, tenant: Uuid, jti: Uuid) -> Result<bool, ApiError> {
    let mut tx = st.auth_pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await?;
    let found: Option<(Uuid,)> = sqlx::query_as(
        "SELECT jti FROM sessions WHERE tenant_id=$1 AND jti=$2 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(tenant)
    .bind(jti)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(found.is_some())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    // S3-R9 hardening — the emailed magic token must be a dedicated, full-entropy CSPRNG secret with NO
    // embedded timestamp (a UUIDv7 leaks its 48-bit issuance time and spends entropy on it). Assert the
    // `mgc_` prefix + a 64-hex-char (256-bit) random body, and that successive tokens differ.
    #[test]
    fn magic_token_is_high_entropy_random_no_timestamp() {
        let t = new_magic_token();
        let body = t.strip_prefix("mgc_").expect("must carry the mgc_ store-routing prefix");
        assert_eq!(body.len(), 64, "expected 256 bits as 64 hex chars, got {}", body.len());
        assert!(
            body.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "token body must be lowercase hex: {body}"
        );
        assert_ne!(new_magic_token(), new_magic_token(), "tokens must be unique per call");
    }
}
