#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::auth::jwt::{mint_session, resolve_token, verify_session, JwtCfg, SessionClaims, TokenSource};
use app_server::auth::password::{hash_password, needs_rehash, verify_password, VerifyOutcome, DUMMY_HASH};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

fn cfg() -> JwtCfg {
    JwtCfg { secret: b"dev-only-secret-please-change-32bytes!!".to_vec(), iss: "qcue".into(), aud: "qcue-app".into(), ttl_secs: 3600 }
}

// ── Task 8: session JWT + dual extractor (pure-unit; no DB) ─────────────────────────────────
#[test]
fn test_jwt_validates_iss_aud() {
    let c = cfg();
    let claims = SessionClaims::new(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), "owner", uuid::Uuid::now_v7(), &c);
    let tok = mint_session(&claims, &c).unwrap();
    assert!(verify_session(&tok, &c).is_ok());
    // foreign aud rejected
    let evil = JwtCfg { aud: "other".into(), ..c };
    assert!(verify_session(&tok, &evil).is_err());
}

#[test]
fn test_dual_extractor() {
    // header first, then ?token= (S3-R12)
    assert!(matches!(resolve_token(Some("Bearer abc"), None, true), Some((TokenSource::Header, t)) if t == "abc"));
    assert!(matches!(resolve_token(None, Some("xyz"), true), Some((TokenSource::Query, t)) if t == "xyz"));
    assert!(resolve_token(None, None, true).is_none());
}

#[test]
fn test_query_token_sse_only() {
    // ?token= accepted only on GET SSE routes (S3-R13); rejected on a mutating route
    assert!(resolve_token(None, Some("xyz"), true).is_some(), "sse route allows ?token=");
    assert!(resolve_token(None, Some("xyz"), false).is_none(), "non-sse route rejects ?token=");
}

// ── AUTH-R2: access TTL is a Config knob (default 3600), parsed from env ────────────────────
#[test]
fn test_config_carries_access_ttl() {
    use app_server::config::Config;
    // default unchanged
    let c = Config::validate(Config::test_raw()).unwrap();
    assert_eq!(c.access_ttl_secs, 3600, "default access TTL stays 3600s");
    // a custom raw value flows through validate untouched
    let mut raw = Config::test_raw();
    raw.access_ttl_secs = 7200;
    let c2 = Config::validate(raw).unwrap();
    assert_eq!(c2.access_ttl_secs, 7200);
}

// ── Task 9: password argon2id constant-time + rehash (pure-unit) ────────────────────────────
#[test]
fn test_login_constant_time_unknown_email() {
    let h = hash_password("correct horse").unwrap();
    let known_bad = verify_password(Some(&h), "wrong");
    let unknown = verify_password(None, "anything"); // None ⇒ verify against DUMMY_HASH
    assert!(matches!(known_bad, VerifyOutcome::Mismatch));
    assert!(matches!(unknown, VerifyOutcome::Mismatch));
    assert!(DUMMY_HASH.starts_with("$argon2id$"));
}

#[test]
fn test_login_success_and_rehash() {
    let h = hash_password("s3cret").unwrap();
    assert!(matches!(verify_password(Some(&h), "s3cret"), VerifyOutcome::Match));
    let stale = "$argon2id$v=19$m=4096,t=1,p=1$c29tZXNhbHRzb21lc2FsdA$RdescudvJCsgt3ub+b+dWRWJTmaaJObG";
    assert!(needs_rehash(stale));
    assert!(!needs_rehash(&h));
}

// ── Task 8: dual extractor accepts header AND ?token= against a real router + DB ─────────────
#[sqlx::test(migrations = "../migrations")]
async fn test_extractor_accepts_header_and_query_token(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "ext-a").await;
    let access = issue_access(&db, tid, uid).await;

    // (a) header path on a protected non-SSE route → 200
    let ok = app
        .clone()
        .oneshot(
            Request::get("/v1/captures")
                .header("authorization", format!("Bearer {access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK, "Bearer header accepted on protected route");

    // (b) ?token= on a non-SSE route → 401 (query token only valid on SSE GET routes, S3-R13)
    let denied = app
        .clone()
        .oneshot(Request::get(format!("/v1/captures?token={access}")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED, "?token= rejected on non-SSE route");

    // (c) ?token= on an SSE GET route → accepted (passes the extractor; 404 because the route isn't
    //     wired in the skeleton, but crucially NOT 401 — the dual extractor authenticated it).
    let sse = app
        .clone()
        .oneshot(
            Request::get(format!("/v1/recall/00000000-0000-7000-8000-000000000000/stream?token={access}"))
                .header("origin", "http://localhost:3000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(sse.status(), StatusCode::UNAUTHORIZED, "?token= authenticates an SSE GET route");
}

// ── Task 8: RLS GUC isolates the extractor's tenant binding ─────────────────────────────────
#[sqlx::test(migrations = "../migrations")]
async fn test_extractor_sets_guc(pool: PgPool) {
    use app_server::tenancy::open_tenant_tx;
    let db = from_pool(pool);
    let (a, _ua) = seed_tenant(&db, "guc-a").await;
    let mut ctx_tx = open_tenant_tx(&db.app, a).await.unwrap();
    let got: uuid::Uuid = sqlx::query_scalar("SELECT app_tenant()").fetch_one(&mut *ctx_tx).await.unwrap();
    assert_eq!(got, a, "extractor must SET LOCAL app.tenant_id to the JWT tenant");
    ctx_tx.commit().await.unwrap();
    // S3-R2: after commit a fresh checkout sees the GUC unset (SET LOCAL is tx-scoped)
    let mut fresh = db.app.begin().await.unwrap();
    let unset: Option<uuid::Uuid> = sqlx::query_scalar("SELECT app_tenant()").fetch_one(&mut *fresh).await.unwrap();
    assert!(unset.is_none(), "SET LOCAL must not leak across pooled connections (S3-R2)");
}

// ── AUTH-R1: refresh revokes ONLY the presented refresh jti; siblings stay live ─────────────
#[sqlx::test(migrations = "../migrations")]
async fn test_refresh_revokes_only_presented_jti(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "rot-a").await;
    // Session X: the refresh chain we will rotate.
    let (refresh_x, jti_x) = issue_refresh_returning_jti(&db, tid, uid).await;
    // Session Y: an independent live session (a second device) — must survive the refresh.
    let (_refresh_y, jti_y) = issue_refresh_returning_jti(&db, tid, uid).await;

    let res = app
        .clone()
        .oneshot(
            Request::post("/v1/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"refresh_token":"{refresh_x}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // The rotated refresh jti (X) is now revoked …
    assert!(
        session_revoked_at(&db, tid, jti_x).await.is_some(),
        "the presented refresh jti must be revoked after rotation"
    );
    // … but the sibling device's session (Y) stays live (no all-sessions cascade).
    assert!(
        session_revoked_at(&db, tid, jti_y).await.is_none(),
        "a sibling session MUST remain live after a refresh on a different jti (AUTH-R1)"
    );
}

// ── AUTH-R1 / S3-R15: the rotated refresh token is one-time-use (its jti is dead) ───────────
#[sqlx::test(migrations = "../migrations")]
async fn test_rotated_refresh_token_is_dead(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, uid) = seed_tenant(&db, "rot-b").await;
    let refresh = issue_refresh(&db, tid, uid).await;

    // first refresh succeeds …
    let ok = app
        .clone()
        .oneshot(
            Request::post("/v1/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"refresh_token":"{refresh}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);

    // … re-presenting the SAME (now-rotated) refresh token → 401 (one-time-use per chain).
    let denied = app
        .clone()
        .oneshot(
            Request::post("/v1/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"refresh_token":"{refresh}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}

// ── Task 10: magic-link single-use + unknown email no-enumeration ───────────────────────────
#[sqlx::test(migrations = "../migrations")]
async fn test_magic_link_single_use_and_unknown_email(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (_t, _u) = seed_tenant(&db, "magic-a").await;

    // unknown email → 200 (no enumeration)
    let res = app
        .clone()
        .oneshot(
            Request::post("/v1/auth/magic/request")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"email":"nobody@example.com"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // a real request + verify works once; a second verify → 401
    let token = request_magic(&db, "magic-a@example.com").await;
    let v1 = app
        .clone()
        .oneshot(
            Request::post("/v1/auth/magic/verify")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"token":"{token}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(v1.status(), StatusCode::OK);
    let v2 = app
        .clone()
        .oneshot(
            Request::post("/v1/auth/magic/verify")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"token":"{token}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(v2.status(), StatusCode::UNAUTHORIZED);
}

// ── Task 10: a valid login still returns 200 even when the audit insert fails (S3-R16) ──────
#[sqlx::test(migrations = "../migrations")]
async fn test_audit_never_blocks_auth(pool: PgPool) {
    let db = from_pool(pool);
    set_audit_fail(true); // force every audit INSERT to error → must be swallowed, never blocking auth
    let app = test_router(&db).await;
    let (tid, _u) = seed_tenant(&db, "audit-a").await;
    set_password(&db, tid, "audit-a@example.com", "pw123456").await;
    let res = app
        .oneshot(
            Request::post("/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"email":"audit-a@example.com","password":"pw123456"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    set_audit_fail(false);
    assert_eq!(res.status(), StatusCode::OK, "a forced-failing audit must not block a valid login");
    // and no audit row was written while the writer was forced to fail (the INSERT errored + swallowed)
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE tenant_id=$1 AND action='auth.login.ok'",
    )
    .bind(tid)
    .fetch_one(&db.migrator)
    .await
    .unwrap();
    assert_eq!(n, 0, "forced-fail audit writes nothing");
}

// ── Task 10: the audit writer now actually lands a row on the happy path (audit_log exists) ──
#[sqlx::test(migrations = "../migrations")]
async fn test_audit_row_lands(pool: PgPool) {
    let db = from_pool(pool);
    let app = test_router(&db).await;
    let (tid, _u) = seed_tenant(&db, "audit-ok").await;
    set_password(&db, tid, "audit-ok@example.com", "pw123456").await;
    let res = app
        .oneshot(
            Request::post("/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"email":"audit-ok@example.com","password":"pw123456"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    // the RLS-bound INSERT lands an `auth.login.ok` audit row for this tenant.
    let mut tx = db.migrator.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tid.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action='auth.login.ok'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(n, 1, "a successful login writes exactly one auth.login.ok audit row");
}

// ── Task 10: social verify fails closed on a malformed/foreign token (no network) ───────────
#[tokio::test]
async fn test_social_verify_rejects_bad_aud() {
    use app_server::auth::social::{verify_id_token, Jwks, SocialCfg};
    let cfg = SocialCfg { google_auds: vec!["qcue-client".into()], apple_auds: vec![] };
    let r = verify_id_token("google", "not.a.jwt", &cfg, &Jwks::new()).await;
    assert!(r.is_err());
}

// ── AUTH-R2: a custom access TTL flows into the minted token exp + sessions.expires_at ───────
#[sqlx::test(migrations = "../migrations")]
async fn test_issued_access_exp_uses_config_ttl(pool: PgPool) {
    use app_server::auth::jwt::{verify_session, JwtCfg};
    use app_server::router::build_router;
    let db = from_pool(pool);
    // Build AppState with a 120s access TTL instead of the 3600s default.
    let mut state = app_state(&db);
    let mut cfg = (*state.cfg).clone();
    cfg.access_ttl_secs = 120;
    state.cfg = std::sync::Arc::new(cfg);
    let app = build_router(state);

    let (tid, _u) = seed_tenant(&db, "ttl-a").await;
    set_password(&db, tid, "ttl-a@example.com", "pw123456").await;

    let res = app
        .oneshot(
            Request::post("/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"email":"ttl-a@example.com","password":"pw123456"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let access = v["access_jwt"].as_str().unwrap();

    // The minted access token's exp ≈ now + 120s (window allows test latency).
    let cfg = JwtCfg {
        secret: b"dev-only-secret-please-change-32bytes!!".to_vec(),
        iss: "qcue".into(),
        aud: "qcue-app".into(),
        ttl_secs: 120,
    };
    let claims = verify_session(access, &cfg).unwrap();
    let life = claims.exp - claims.iat;
    assert_eq!(life, 120, "minted access token lifetime must equal the configured TTL");
}

// ── NG-R7: GOOGLE_OAUTH_AUDIENCES parses into Config.google_oauth_audiences ──────────────────
#[test]
fn test_config_parses_google_audiences() {
    use app_server::config::Config;
    let mut raw = Config::test_raw();
    raw.google_oauth_audiences = vec!["web-aud".into(), "ios-aud".into()];
    let cfg = Config::validate(raw).unwrap();
    assert_eq!(cfg.google_oauth_audiences, vec!["web-aud".to_string(), "ios-aud".to_string()]);
}

// ── SIWA-R1: APPLE_OAUTH_AUDIENCES parses into Config.apple_oauth_audiences ───────────────────
#[test]
fn test_config_parses_apple_audiences() {
    use app_server::config::Config;
    let mut raw = Config::test_raw();
    raw.apple_oauth_audiences = vec!["cn.qcue.app".into()];
    let cfg = Config::validate(raw).unwrap();
    assert_eq!(cfg.apple_oauth_audiences, vec!["cn.qcue.app".to_string()]);
}

// ── NG-R1..R6: Google id_token verify against an injected JWKS key (no network) ──────────────
// A throwaway RSA test keypair (test-only constants — NOT a real Google key).
#[cfg(test)]
const TEST_RSA_PRIV: &str = "-----BEGIN PRIVATE KEY-----
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQCeSk/OtRbDFxCD
ttpeVte4KKhoTJWiboTQvqtTuQDgmK04lbh04c3fgE3TSmqAM+Xkm028CbVUVipq
I0Q0aq3lJHSqg49o7Y1zqC9TjyDaVvxvF1a6BUr6+1QvqAwGPPiVJNuN1YX7pZrS
dkuz2SDOPgpj3nl9JUZRBSO2BYHAV6GfbfbYA3hUZlk88k2e7p1wOuiDSbDtLoP9
QiM2Qp5Qlt4r9thJ1/3EPz1gOufWB0knKjbaAYzA+rlr2P+LMhBowYAyW9Imrbq+
wTBPPIucoryPZEnM/lFiIYvcugMMBl4twemjJYzZvbi8MgVsp9Xrid9EosbVju5A
Z28nHhzNAgMBAAECggEAEecVOiuMmMGPzlncvk1DpjJBA9Tfmqi10Fc0UOqMcRqL
guoaG+wbCQN9qd9RhtD32CCBjPorHlAFiY4WDXigVNmH8W4iRuuRM0rLGYAHZvJu
KBFjb3QgTB0nYyF2RLFaKyIpS9QhzHmpNlMHUl7FIVZufeegZXlVB95VMOXUDEEf
fjhOBfAflriOCRio6PCJZpeSJTWkbQ/iIa4c7dcBmgzr0i4IebZtRIacG2wenm1T
zO6wGacxBjoavo1LoTJx3xGepL53nR66QvuNGFmmL3RAbnB+XFL/8XiSlJjSiyff
+2ahu7oY2njPLOks+gP++vOw4k8GtU9ZpbfvQ7PRyQKBgQDMAatPIFMwfLYvOGC6
hum0oalIc4MJ2sp9e9r6o31cGNkh/5JYiCDa57yvn/g6bdHa60/B+BjXak24K+EB
BS3ijOuJV2WWkGfVvYKtqfb1WoKM/Iwi1XsXE8zhsTRJ0+BC0Dk9NIUBX2COPgJ7
a+XEZUzUnapcVzlOSJfO+PWvpQKBgQDGoec4zQTe2HOeq+DhhCheGaVCnGcFv8Zx
FfUNPROpHroEoSJUIKCpYKvxNYQL8WkVA8Gff500E5LjDwuLdlwTExHwl2uLRTyH
xcBVdO/YuNVAkWwZpEYLSv0aGZVhohhfnnpCGKbudbi+2XS65IiHfR1ZvjD2OGTo
vp90iZwwCQKBgGa3uG/A0OIrCPhBpMKGR4oBk+C8+I+vsCD6icmFJAuJH1r0+dTF
xfUylVjAbRXOUcmujZwWtTtRdQx0W3hOCUp2temTLb1fvEhsgS271HK5Pd6LEmw/
nRiDibdhp/g8TECX4xokJYwJX+5+3nUSYMBAWSz8rdiMunfmKTm3NM1ZAoGALf1y
kd42UHqBWq2lJdH5nsAFWYTo/ZXHlotk76nCkZfBriy4zA255T2y0eh4KGO+1tTF
0e40MciOa/Ah1iqTav8xWilVBywCtdT9kUu/9Mfm6EpDYzR720WDkLV3tuFXD1yc
Jg2bKP8sxVOICXW8ftJjJ1I39+pawuDP/qWV+jECgYBEOFZJU3gq4Tri7d0VKqf6
wXPKinWVCPO+pCsP8/BDo0OjsamId5t7NA3uhjueko1y1Da0VuBe7IplDNYAhjbE
2h7bMVGL2taMDoSHc/rGVEfD9m2LPEXwb9rT0KSDChFDRJ+U6KSFZ3P2GZK1ap8H
vCxvFzSitOKuIHqi6/zBww==
-----END PRIVATE KEY-----";

#[cfg(test)]
const TEST_RSA_PUB: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAnkpPzrUWwxcQg7baXlbX
uCioaEyVom6E0L6rU7kA4JitOJW4dOHN34BN00pqgDPl5JtNvAm1VFYqaiNENGqt
5SR0qoOPaO2Nc6gvU48g2lb8bxdWugVK+vtUL6gMBjz4lSTbjdWF+6Wa0nZLs9kg
zj4KY955fSVGUQUjtgWBwFehn2322AN4VGZZPPJNnu6dcDrog0mw7S6D/UIjNkKe
UJbeK/bYSdf9xD89YDrn1gdJJyo22gGMwPq5a9j/izIQaMGAMlvSJq26vsEwTzyL
nKK8j2RJzP5RYiGL3LoDDAZeLcHpoyWM2b24vDIFbKfV64nfRKLG1Y7uQGdvJx4c
zQIDAQAB
-----END PUBLIC KEY-----";

#[cfg(test)]
fn mint_test_google_token(aud: &str, iss: &str, email_verified: bool, exp_offset_secs: i64) -> String {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    let now = chrono::Utc::now().timestamp();
    let claims = serde_json::json!({
        "sub": "google-subject-123",
        "email": "helios@sinox.ai",
        "email_verified": email_verified,
        "iss": iss,
        "aud": aud,
        "exp": now + exp_offset_secs,
        "iat": now,
    });
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-kid".to_string());
    encode(&header, &claims, &EncodingKey::from_rsa_pem(TEST_RSA_PRIV.as_bytes()).unwrap()).unwrap()
}

/// Mint an Apple identity token. `email_verified` is a `serde_json::Value` so a test can pass either a
/// real bool (`json!(true)`) or Apple's string form (`json!("true")`).
#[cfg(test)]
fn mint_test_apple_token(aud: &str, email_verified: serde_json::Value, exp_offset_secs: i64) -> String {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    let now = chrono::Utc::now().timestamp();
    let claims = serde_json::json!({
        "sub": "apple-subject-abc",
        "email": "helios@privaterelay.appleid.com",
        "email_verified": email_verified,
        "iss": "https://appleid.apple.com",
        "aud": aud,
        "exp": now + exp_offset_secs,
        "iat": now,
    });
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-kid".to_string());
    encode(&header, &claims, &EncodingKey::from_rsa_pem(TEST_RSA_PRIV.as_bytes()).unwrap()).unwrap()
}

#[cfg(test)]
fn test_jwks() -> app_server::auth::social::Jwks {
    use jsonwebtoken::DecodingKey;
    app_server::auth::social::Jwks::with_test_key(
        "test-kid",
        DecodingKey::from_rsa_pem(TEST_RSA_PUB.as_bytes()).unwrap(),
    )
}

#[tokio::test]
async fn test_social_verify_accepts_valid_google_token() {
    use app_server::auth::social::{verify_id_token, SocialCfg};
    let cfg = SocialCfg { google_auds: vec!["test-aud".into()], apple_auds: vec![] };
    let tok = mint_test_google_token("test-aud", "https://accounts.google.com", true, 3600);
    let claims = verify_id_token("google", &tok, &cfg, &test_jwks()).await.unwrap();
    assert_eq!(claims.sub, "google-subject-123");
    assert_eq!(claims.email.as_deref(), Some("helios@sinox.ai"));
}

#[tokio::test]
async fn test_social_verify_accepts_bare_google_issuer() {
    use app_server::auth::social::{verify_id_token, SocialCfg};
    let cfg = SocialCfg { google_auds: vec!["test-aud".into()], apple_auds: vec![] };
    let tok = mint_test_google_token("test-aud", "accounts.google.com", true, 3600);
    assert!(verify_id_token("google", &tok, &cfg, &test_jwks()).await.is_ok());
}

#[tokio::test]
async fn test_social_verify_rejects_wrong_aud_not_in_list() {
    use app_server::auth::social::{verify_id_token, SocialCfg};
    let cfg = SocialCfg { google_auds: vec!["expected-aud".into()], apple_auds: vec![] };
    let tok = mint_test_google_token("attacker-aud", "https://accounts.google.com", true, 3600);
    assert!(verify_id_token("google", &tok, &cfg, &test_jwks()).await.is_err());
}

#[tokio::test]
async fn test_social_verify_rejects_expired() {
    use app_server::auth::social::{verify_id_token, SocialCfg};
    let cfg = SocialCfg { google_auds: vec!["test-aud".into()], apple_auds: vec![] };
    // -3600 clears jsonwebtoken's default 60s exp leeway, so the token is unambiguously expired.
    let tok = mint_test_google_token("test-aud", "https://accounts.google.com", true, -3600);
    assert!(verify_id_token("google", &tok, &cfg, &test_jwks()).await.is_err());
}

#[tokio::test]
async fn test_social_verify_rejects_unverified_email() {
    use app_server::auth::social::{verify_id_token, SocialCfg};
    let cfg = SocialCfg { google_auds: vec!["test-aud".into()], apple_auds: vec![] };
    let tok = mint_test_google_token("test-aud", "https://accounts.google.com", false, 3600);
    assert!(verify_id_token("google", &tok, &cfg, &test_jwks()).await.is_err());
}

// ── SIWA-R1: Apple identity-token verification (iss=appleid.apple.com, aud=bundle id) ──────────
#[tokio::test]
async fn test_social_verify_accepts_valid_apple_token() {
    use app_server::auth::social::{verify_id_token, SocialCfg};
    let cfg = SocialCfg { google_auds: vec![], apple_auds: vec!["cn.qcue.app".into()] };
    let tok = mint_test_apple_token("cn.qcue.app", serde_json::json!(true), 3600);
    let claims = verify_id_token("apple", &tok, &cfg, &test_jwks()).await.unwrap();
    assert_eq!(claims.sub, "apple-subject-abc");
    assert_eq!(claims.email.as_deref(), Some("helios@privaterelay.appleid.com"));
}

// Apple sometimes encodes email_verified as the STRING "true" — must still deserialize + verify.
#[tokio::test]
async fn test_social_verify_apple_email_verified_as_string() {
    use app_server::auth::social::{verify_id_token, SocialCfg};
    let cfg = SocialCfg { google_auds: vec![], apple_auds: vec!["cn.qcue.app".into()] };
    let tok = mint_test_apple_token("cn.qcue.app", serde_json::json!("true"), 3600);
    assert!(verify_id_token("apple", &tok, &cfg, &test_jwks()).await.is_ok());
}

// An Apple token whose aud is not in the allow-list is rejected.
#[tokio::test]
async fn test_social_verify_rejects_apple_wrong_aud() {
    use app_server::auth::social::{verify_id_token, SocialCfg};
    let cfg = SocialCfg { google_auds: vec![], apple_auds: vec!["cn.qcue.app".into()] };
    let tok = mint_test_apple_token("com.attacker.app", serde_json::json!(true), 3600);
    assert!(verify_id_token("apple", &tok, &cfg, &test_jwks()).await.is_err());
}

// An empty apple_auds (APPLE_OAUTH_AUDIENCES unset → Apple disabled) rejects every Apple token.
#[tokio::test]
async fn test_social_verify_apple_disabled_when_no_aud() {
    use app_server::auth::social::{verify_id_token, SocialCfg};
    let cfg = SocialCfg { google_auds: vec![], apple_auds: vec![] };
    let tok = mint_test_apple_token("cn.qcue.app", serde_json::json!(true), 3600);
    assert!(verify_id_token("apple", &tok, &cfg, &test_jwks()).await.is_err());
}

// An Apple token whose email is present but UNVERIFIED (e.g. a managed "Work & School" id whose domain
// the org never verified) is rejected — otherwise it could link/adopt another user's account by email
// (account pre-hijacking guard; matches the Google email_verified requirement).
#[tokio::test]
async fn test_social_verify_rejects_apple_unverified_email() {
    use app_server::auth::social::{verify_id_token, SocialCfg};
    let cfg = SocialCfg { google_auds: vec![], apple_auds: vec!["cn.qcue.app".into()] };
    let tok = mint_test_apple_token("cn.qcue.app", serde_json::json!(false), 3600);
    assert!(verify_id_token("apple", &tok, &cfg, &test_jwks()).await.is_err());
}

// NG-R1: the JWKS JSON parser maps kid → key for the live Google certs shape.
#[test]
fn test_parse_google_jwks_maps_kids() {
    use app_server::auth::social::parse_google_jwks;
    // Real RSA modulus (the test keypair's) so from_rsa_components accepts it; e=AQAB (65537).
    let n = "nkpPzrUWwxcQg7baXlbXuCioaEyVom6E0L6rU7kA4JitOJW4dOHN34BN00pqgDPl5JtNvAm1VFYqaiNENGqt5SR0qoOPaO2Nc6gvU48g2lb8bxdWugVK-vtUL6gMBjz4lSTbjdWF-6Wa0nZLs9kgzj4KY955fSVGUQUjtgWBwFehn2322AN4VGZZPPJNnu6dcDrog0mw7S6D_UIjNkKeUJbeK_bYSdf9xD89YDrn1gdJJyo22gGMwPq5a9j_izIQaMGAMlvSJq26vsEwTzyLnKK8j2RJzP5RYiGL3LoDDAZeLcHpoyWM2b24vDIFbKfV64nfRKLG1Y7uQGdvJx4czQ";
    let v = serde_json::json!({
        "keys": [
            {"kid":"k1","kty":"RSA","alg":"RS256","use":"sig","n":n,"e":"AQAB"},
            {"kid":"k2","kty":"RSA","alg":"RS256","use":"sig","n":n,"e":"AQAB"}
        ]
    });
    let m = parse_google_jwks(&v);
    assert!(m.contains_key("k1") && m.contains_key("k2"));
}

// ── NG-R8..R10: POST /v1/auth/social verifies a Google id_token, creates user+oauth_identity, issues a session
#[sqlx::test(migrations = "../migrations")]
async fn test_social_google_creates_user_and_session(pool: PgPool) {
    use app_server::router::build_router;
    use jsonwebtoken::DecodingKey;
    let db = from_pool(pool);
    let mut state = app_state(&db);
    // audience allow-list + injected JWKS test key (no network).
    let mut cfg = (*state.cfg).clone();
    cfg.google_oauth_audiences = vec!["test-aud".into()];
    state.cfg = std::sync::Arc::new(cfg);
    state.jwks = std::sync::Arc::new(app_server::auth::social::Jwks::with_test_key(
        "test-kid",
        DecodingKey::from_rsa_pem(TEST_RSA_PUB.as_bytes()).unwrap(),
    ));
    let app = build_router(state);

    let tok = mint_test_google_token("test-aud", "https://accounts.google.com", true, 3600);
    let res = app
        .oneshot(
            Request::post("/v1/auth/social")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"provider":"google","id_token":"{tok}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert!(v["access_jwt"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(v["refresh_jwt"].as_str().is_some_and(|s| !s.is_empty()));

    // a users row + an oauth_identities row now exist for this Google identity.
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE email='helios@sinox.ai'")
        .fetch_one(&db.migrator).await.unwrap();
    assert_eq!(users, 1);
    let oid: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM oauth_identities WHERE provider='google' AND subject='google-subject-123'")
        .fetch_one(&db.migrator).await.unwrap();
    assert_eq!(oid, 1);
}

// NG-R8: a second sign-in with the SAME Google subject reuses the user (no duplicate).
#[sqlx::test(migrations = "../migrations")]
async fn test_social_google_returning_subject_is_idempotent(pool: PgPool) {
    use app_server::router::build_router;
    use jsonwebtoken::DecodingKey;
    let db = from_pool(pool);
    let mk = |db: &TestDb| {
        let mut state = app_state(db);
        let mut cfg = (*state.cfg).clone();
        cfg.google_oauth_audiences = vec!["test-aud".into()];
        state.cfg = std::sync::Arc::new(cfg);
        state.jwks = std::sync::Arc::new(app_server::auth::social::Jwks::with_test_key(
            "test-kid", DecodingKey::from_rsa_pem(TEST_RSA_PUB.as_bytes()).unwrap()));
        build_router(state)
    };
    let tok = mint_test_google_token("test-aud", "https://accounts.google.com", true, 3600);
    for _ in 0..2 {
        let res = mk(&db).oneshot(
            Request::post("/v1/auth/social")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"provider":"google","id_token":"{tok}"}}"#))).unwrap(),
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE email='helios@sinox.ai'")
        .fetch_one(&db.migrator).await.unwrap();
    assert_eq!(users, 1, "the same Google subject must not create a second user");
}
