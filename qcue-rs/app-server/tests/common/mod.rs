// QCue S3 test fixture — a throwaway Postgres (via `#[sqlx::test(migrations = "../migrations")]`)
// + Appendix B migrations + the auth/global test router (Master §11 M0).
//
// DEVIATION FROM PLAN (recorded): the plan's fixture hand-rolls a migration runner and provisions
// the dedicated `qcue_app`/`qcue_auth`/`qcue_migrator` roles. In THIS environment the connection
// role (`qcue`) lacks CREATEROLE, so those roles can't be created and the migrations' permission-
// tolerant DO-blocks skip role creation. We therefore reuse the SINGLE `qcue` role (which is
// NOSUPERUSER + NOBYPASSRLS — so RLS still bites) for the app and auth pools, and we use sqlx's own
// migrator (the store crate's proven `#[sqlx::test(migrations = "../migrations")]` pattern) which
// gives every test a fresh, tenant-empty database. Tests take the sqlx-injected `PgPool` and build
// a `TestDb` via `from_pool`.
#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use app_server::auth::jwt::{mint_session, JwtCfg, SessionClaims};
use app_server::config::Config;
use app_server::objstore::ObjStore;
use app_server::state::AppState;
use app_server::tenancy::TenantTx;
use app_server::vault::secrets::{KmsSecrets, Secrets};
use axum::response::Response;
use axum::Router;
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use sqlx::PgPool;
use uuid::Uuid;

/// Forces the audit writer to fail, exercising the "audit never blocks auth" invariant (S3-R16).
/// Delegates to the real test seam inside `app-server` (the audit writer now performs a real INSERT).
pub fn set_audit_fail(v: bool) {
    app_server::auth::audit::set_audit_fail(v);
}

pub struct TestDb {
    /// In this single-role environment all three are the same `qcue` pool (see module note).
    pub app: PgPool,
    pub auth: PgPool,
    pub migrator: PgPool,
}

/// Build a `TestDb` from the sqlx-injected pool (migrations already applied by `#[sqlx::test]`).
pub fn from_pool(pool: PgPool) -> TestDb {
    app_server::auth::audit::set_audit_fail(false);
    TestDb { app: pool.clone(), auth: pool.clone(), migrator: pool }
}

/// Test JWT config matching the auth routes' issuer/audience.
pub fn jwt_cfg() -> JwtCfg {
    JwtCfg {
        secret: b"dev-only-secret-please-change-32bytes!!".to_vec(),
        iss: "qcue".into(),
        aud: "qcue-app".into(),
        ttl_secs: 3600,
    }
}

/// The test config (loopback, gates off) used to build `AppState`.
pub fn test_config() -> Config {
    Config::validate(Config::test_raw()).expect("valid test config")
}

pub fn app_state(db: &TestDb) -> AppState {
    app_state_at(db, &test_data_root())
}

/// Build `AppState` pinned to a KNOWN object/vault data root (so a test can seed wiki bodies under the
/// same `<root>/objects/t/<tenant>/u/_` the page-read endpoint reads from). `cfg.data_root` is set to it.
pub fn app_state_at(db: &TestDb, data_root: &str) -> AppState {
    let mut cfg = test_config();
    cfg.data_root = data_root.to_string();
    AppState {
        cfg: Arc::new(cfg),
        pool: db.app.clone(),
        auth_pool: db.auth.clone(),
        secrets: stub_secrets(),
        objstore: Arc::new(ObjStore::new(data_root)),
        threads: app_server::wire::hub::StreamHub::new(),
        dream_streams: app_server::wire::hub::StreamHub::new(),
        // keyless recall path: a stub-backed RouterWikiLlm that synthesizes a short answer with a
        // `## References` block so the recall SSE taxonomy (citation*) is exercisable without keys.
        recall_llm: Arc::new(app_server::ingest::RouterWikiLlm::stub(
            "Answer from the wiki.\n\n## References\n- [[rust]] — the rust page",
        )),
        // keyless plain extraction seam (no tools) — mirrors production's separate ingest_llm.
        ingest_llm: Arc::new(app_server::ingest::RouterWikiLlm::stub("{\"fully_redundant\":false}")),
        // keyless voice transcription seam — returns a fixed transcript so the route is testable.
        transcriber: Arc::new(app_server::transcribe::StubTranscriber::new(STUB_TRANSCRIPT)),
        jwks: Arc::new(app_server::auth::social::Jwks::new()),
    }
}

/// The canned transcript the keyless `StubTranscriber` returns in tests (see `transcribe_route.rs`).
pub const STUB_TRANSCRIPT: &str = "stubbed voice transcript";

/// A throwaway per-process data root for the object store (capture JSONL lands here).
pub fn test_data_root() -> String {
    let root = std::env::temp_dir().join(format!("qcue-test-data/{}", Uuid::now_v7()));
    let _ = std::fs::create_dir_all(&root);
    root.to_string_lossy().to_string()
}

/// The keyless `Secrets` test double, backed by the S1 `secrets::StubKms` (deterministic envelope so
/// `open(seal(x)) == x`). Real KMS slots in unchanged.
pub fn stub_secrets() -> Arc<dyn Secrets> {
    Arc::new(KmsSecrets::new(Arc::new(secrets::StubKms::new()), "kek-test"))
}

/// Build the full test router (auth routes + health + global middleware + a protected probe).
pub async fn test_router(db: &TestDb) -> Router {
    app_server::router::build_router(app_state(db))
}

/// Build the full test router pinned to a KNOWN data root (for the wiki body-read endpoint test).
pub async fn test_router_at(db: &TestDb, data_root: &str) -> Router {
    app_server::router::build_router(app_state_at(db, data_root))
}

/// Seed one tenant + one owner user; returns (tenant_id, user_id).
///
/// `tenants` is the RLS-free root, so its insert needs no GUC. `users` has FORCE RLS, so (under the
/// single non-bypass `qcue` role) the seed binds `app.tenant_id` inside the tx before inserting.
pub async fn seed_tenant(db: &TestDb, slug: &str) -> (Uuid, Uuid) {
    let tid = Uuid::now_v7();
    let ns = format!("t/{}", Uuid::now_v7());
    sqlx::query("INSERT INTO tenants(id, slug, display_name, namespace) VALUES ($1,$2,$2,$3)")
        .bind(tid)
        .bind(slug)
        .bind(&ns)
        .execute(&db.migrator)
        .await
        .unwrap();
    let uid = Uuid::now_v7();
    let mut tx = db.migrator.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tid.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users(id, tenant_id, email) VALUES ($1,$2,$3)")
        .bind(uid)
        .bind(tid)
        .bind(format!("{slug}@example.com"))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    (tid, uid)
}

/// Set a known argon2id password hash for a user (so login can succeed). UPDATE on the RLS-forced
/// `users` table needs the tenant GUC bound in-tx.
pub async fn set_password(db: &TestDb, tenant: Uuid, email: &str, pw: &str) {
    let hash = app_server::auth::password::hash_password(pw).unwrap();
    let mut tx = db.migrator.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET password_hash=$1 WHERE email=$2")
        .bind(hash)
        .bind(email)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

/// Mint an access JWT AND insert the live sessions row (so the extractor's revocation gate passes).
pub async fn issue_access(db: &TestDb, tenant: Uuid, user: Uuid) -> String {
    let c = jwt_cfg();
    let jti = Uuid::now_v7();
    let claims = SessionClaims::new(user, tenant, "owner", jti, &c);
    let token = mint_session(&claims, &c).unwrap();
    let exp = Utc::now() + Duration::hours(1);
    // sessions has FORCE RLS, so set the GUC inside the seed tx.
    let mut tx = db.migrator.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions(tenant_id,user_id,jti,expires_at) VALUES ($1,$2,$3,$4)")
        .bind(tenant)
        .bind(user)
        .bind(jti)
        .bind(exp)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    token
}

/// Mint a refresh token (a long-lived JWT with `aud=qcue-refresh`) + its sessions row.
pub async fn issue_refresh(db: &TestDb, tenant: Uuid, user: Uuid) -> String {
    let mut c = jwt_cfg();
    c.aud = "qcue-refresh".into();
    c.ttl_secs = 60 * 60 * 24 * 30;
    let jti = Uuid::now_v7();
    let claims = SessionClaims::new(user, tenant, "owner", jti, &c);
    let token = mint_session(&claims, &c).unwrap();
    let exp = Utc::now() + Duration::days(30);
    let mut tx = db.migrator.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions(tenant_id,user_id,jti,expires_at) VALUES ($1,$2,$3,$4)")
        .bind(tenant)
        .bind(user)
        .bind(jti)
        .bind(exp)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    token
}

/// Like [`issue_refresh`] but also returns the minted refresh `jti` so a test can
/// assert that exactly THAT session row was revoked (or left live).
pub async fn issue_refresh_returning_jti(db: &TestDb, tenant: Uuid, user: Uuid) -> (String, Uuid) {
    let mut c = jwt_cfg();
    c.aud = "qcue-refresh".into();
    c.ttl_secs = 60 * 60 * 24 * 30;
    let jti = Uuid::now_v7();
    let claims = SessionClaims::new(user, tenant, "owner", jti, &c);
    let token = mint_session(&claims, &c).unwrap();
    let exp = Utc::now() + Duration::days(30);
    let mut tx = db.migrator.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions(tenant_id,user_id,jti,expires_at) VALUES ($1,$2,$3,$4)")
        .bind(tenant)
        .bind(user)
        .bind(jti)
        .bind(exp)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    (token, jti)
}

/// Read a session row's `revoked_at` (NULL ⇒ still live). RLS-bound read on the auth pool.
pub async fn session_revoked_at(
    db: &TestDb,
    tenant: Uuid,
    jti: Uuid,
) -> Option<chrono::DateTime<Utc>> {
    let mut tx = db.migrator.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let v: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT revoked_at FROM sessions WHERE tenant_id=$1 AND jti=$2")
            .bind(tenant)
            .bind(jti)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    v
}

/// Request a magic-link token for `email` directly via the route layer, returning the raw token.
/// The route stashes the single-use token in the in-process magic store; we read it back here.
pub async fn request_magic(db: &TestDb, email: &str) -> String {
    app_server::auth::routes::test_issue_magic(&db.auth, email)
        .await
        .expect("magic token issued for known email")
}

// ── jobs / vault test helpers ───────────────────────────────────────────────────────────────

/// Open a request-scoped tx on the app pool with `app.tenant_id` bound (the queue enqueue takes one).
pub async fn tenant_tx(db: &TestDb, tenant: Uuid) -> TenantTx {
    app_server::tenancy::open_tenant_tx(&db.app, tenant)
        .await
        .expect("open tenant tx")
}

/// Simulate a saturated per-tenant in-flight budget by pre-inserting `n` pending jobs for the tenant,
/// so the next `enqueue` sees `count >= MAX_PENDING_PER_TENANT` and returns `Overloaded` (-32001).
pub async fn set_inflight(db: &TestDb, tenant: Uuid, n: i64) {
    let mut tx = tenant_tx(db, tenant).await;
    for _ in 0..n {
        sqlx::query("INSERT INTO jobs(tenant_id,kind,state) VALUES ($1,'lint'::job_kind,'pending'::job_state)")
            .bind(tenant)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
}

/// Drain an axum response body to a String (for asserting the wire shape never leaks a secret).
pub async fn body_string(res: Response) -> String {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).to_string()
}

// ── authed HTTP helpers (Tasks 4-7) ─────────────────────────────────────────────────────────
//
// Reusable Bearer-authed request drivers over the test `Router`, mirroring the
// `app.clone().oneshot(...)` pattern in `capture_idempotency.rs::post_capture`. They take the
// router by ref + an access token + a path, and (for write verbs) a JSON body string, returning the
// raw `Response` so callers can read `res.status()` and `body_string(res)`.

/// POST `path` with a JSON body, Bearer-authed. Returns the raw response.
pub async fn post(app: &Router, path: &str, tok: &str, body: &str) -> Response {
    send(app, axum::http::Method::POST, path, tok, Some(body)).await
}

/// GET `path`, Bearer-authed. Returns the raw response.
pub async fn get(app: &Router, path: &str, tok: &str) -> Response {
    send(app, axum::http::Method::GET, path, tok, None).await
}

/// PATCH `path` with a JSON body, Bearer-authed. Returns the raw response.
pub async fn patch(app: &Router, path: &str, tok: &str, body: &str) -> Response {
    send(app, axum::http::Method::PATCH, path, tok, Some(body)).await
}

/// DELETE `path`, Bearer-authed. Returns the raw response.
pub async fn delete(app: &Router, path: &str, tok: &str) -> Response {
    send(app, axum::http::Method::DELETE, path, tok, None).await
}

/// Shared driver: build a Bearer-authed request (JSON content-type when a body is present) and run it
/// through one `oneshot` against a clone of the router.
async fn send(
    app: &Router,
    method: axum::http::Method,
    path: &str,
    tok: &str,
    body: Option<&str>,
) -> Response {
    use tower::ServiceExt;
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {tok}"));
    let payload = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            axum::body::Body::from(b.to_string())
        }
        None => axum::body::Body::empty(),
    };
    app.clone().oneshot(req.body(payload).unwrap()).await.unwrap()
}

/// Drain the first `n` bytes of an SSE body (the stream is open-ended; we only need the leading frames).
pub async fn sse_prefix(res: Response, n: usize) -> String {
    use futures_util::StreamExt;
    let mut body = res.into_body().into_data_stream();
    let mut out = String::new();
    while out.len() < n {
        match body.next().await {
            Some(Ok(chunk)) => out.push_str(&String::from_utf8_lossy(&chunk)),
            _ => break,
        }
    }
    out
}

// ── wire test helpers (Task 21/22) ──────────────────────────────────────────────────────────

/// Build a minimal `RuntimeEventEnvelope` for the replay-ring tests.
pub fn mk_env(thread_id: Uuid, seq: u64) -> app_server_protocol::RuntimeEventEnvelope {
    app_server_protocol::RuntimeEventEnvelope {
        schema_version: 1,
        thread_id,
        turn_id: None,
        seq,
        event: app_server_protocol::RuntimeEvent::ItemDelta.as_wire().to_string(),
        payload: serde_json::Value::Null,
    }
}

/// Drive a stubbed recall/agent turn and collect the ordered `event` wire kinds (for the streaming
/// invariant test — `item/started → delta* → item/completed`, never a delta after completion).
pub async fn ordered_event_kinds() -> Vec<String> {
    use app_server::wire::engine::Engine;
    use tokio_util::sync::CancellationToken;
    let engine = Engine::new();
    let cancel = CancellationToken::new();
    let mut rx = engine.start_turn_stub(cancel.clone()).await;
    let mut kinds = Vec::new();
    // collect a few deltas then cancel so the turn completes deterministically.
    for _ in 0..5 {
        if let Some(e) = rx.recv().await {
            kinds.push(e.event);
        }
    }
    cancel.cancel();
    while let Some(e) = rx.recv().await {
        kinds.push(e.event);
    }
    kinds
}

// ── S3 read-surface seed helpers (wiki / approvals / jobs / cost) ───────────────────────────

/// Seed a non-deleted `wiki_pages` row and write its markdown body under the tenant vault root so the
/// page-read endpoint can load `body_markdown` root-confined. Returns the new page id. `data_root` must
/// be the SAME root the `AppState` was built with (so the body resolves under `<root>/objects/t/<t>/u/_`).
#[allow(clippy::too_many_arguments)]
pub async fn insert_wiki_page(
    db: &TestDb,
    data_root: &str,
    tenant: Uuid,
    r#type: &str,
    slug: &str,
    title: &str,
    summary: &str,
    body_markdown: &str,
    aliases: &[&str],
    tags: &[&str],
) -> Uuid {
    // write the body under the per-tenant vault root and record its ABSOLUTE path as body_ref.
    let vault = std::path::PathBuf::from(data_root)
        .join("objects")
        .join(format!("t/{tenant}/u/_"));
    std::fs::create_dir_all(&vault).unwrap();
    let abs = vault.join(format!("{slug}.md"));
    std::fs::write(&abs, body_markdown.as_bytes()).unwrap();
    let body_ref = abs.to_string_lossy().to_string();
    let aliases: Vec<String> = aliases.iter().map(|s| s.to_string()).collect();
    let tags: Vec<String> = tags.iter().map(|s| s.to_string()).collect();
    let mut tx = tenant_tx(db, tenant).await;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO wiki_pages (tenant_id,type,slug,title,aliases,tags,summary,char_len,body_ref) \
         VALUES ($1,$2::wiki_page_type,$3,$4,$5,$6,$7,$8,$9) RETURNING id",
    )
    .bind(tenant)
    .bind(r#type)
    .bind(slug)
    .bind(title)
    .bind(&aliases)
    .bind(&tags)
    .bind(summary)
    .bind(i32::try_from(body_markdown.chars().count()).unwrap_or(0))
    .bind(&body_ref)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

/// Seed a resolved incoming link: `src` → `target` (so `target` gets a backlink from `src`).
pub async fn insert_wiki_link(db: &TestDb, tenant: Uuid, src: Uuid, target: Uuid, target_slug: &str) {
    let mut tx = tenant_tx(db, tenant).await;
    sqlx::query(
        "INSERT INTO wiki_links (tenant_id,src_page_id,target_slug,target_page_id) VALUES ($1,$2,$3,$4)",
    )
    .bind(tenant)
    .bind(src)
    .bind(target_slug)
    .bind(target)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// Seed a pending `approvals` row (D13 gate). Returns the approval id.
pub async fn insert_approval(
    db: &TestDb,
    tenant: Uuid,
    user: Uuid,
    action: &str,
    requested_by: &str,
    subject: serde_json::Value,
) -> Uuid {
    let mut tx = tenant_tx(db, tenant).await;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO approvals (tenant_id,user_id,action,subject_ref,requested_by) \
         VALUES ($1,$2,$3,$4,$5) RETURNING id",
    )
    .bind(tenant)
    .bind(user)
    .bind(action)
    .bind(&subject)
    .bind(requested_by)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

/// Seed a `jobs` row with a chosen kind/state (+ optional result json for a progress field). Returns id.
pub async fn insert_job(
    db: &TestDb,
    tenant: Uuid,
    kind: &str,
    state: &str,
    result: Option<serde_json::Value>,
) -> Uuid {
    let mut tx = tenant_tx(db, tenant).await;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO jobs (tenant_id,kind,state,result) VALUES ($1,$2::job_kind,$3::job_state,$4) RETURNING id",
    )
    .bind(tenant)
    .bind(kind)
    .bind(state)
    .bind(&result)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

/// Seed a TENANT-scope `cost_ledger` row for `day` (YYYY-MM-DD) with the 5 token kinds + cost_micros.
#[allow(clippy::too_many_arguments)]
pub async fn insert_cost_row(
    db: &TestDb,
    tenant: Uuid,
    day: &str,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    reasoning: i64,
    cost_micros: i64,
) {
    let mut tx = tenant_tx(db, tenant).await;
    sqlx::query(
        "INSERT INTO cost_ledger \
           (tenant_id,scope,user_id,day,input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,cost_micros) \
         VALUES ($1,'tenant',NULL,$2::date,$3,$4,$5,$6,$7,$8)",
    )
    .bind(tenant)
    .bind(day)
    .bind(input)
    .bind(output)
    .bind(cache_read)
    .bind(cache_write)
    .bind(reasoning)
    .bind(cost_micros)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// Soft-delete a page (simulating the gate's propose-time reversible side) for the approve/reject tests.
pub async fn soft_delete_page(db: &TestDb, tenant: Uuid, id: Uuid) {
    let mut tx = tenant_tx(db, tenant).await;
    sqlx::query("UPDATE wiki_pages SET deleted_at=now() WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

/// Read a page's `deleted_at` (NULL ⇒ live, Some ⇒ soft-deleted) for the reject-restores assertion.
pub async fn page_deleted_at(
    db: &TestDb,
    tenant: Uuid,
    id: Uuid,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let mut tx = tenant_tx(db, tenant).await;
    let v: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM wiki_pages WHERE id=$1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    v
}

/// Seed a provider_credentials row directly (placeholder ciphertext) with a chosen status/cooldown,
/// so the vault list/status surface can be asserted without re-PUTting a key.
pub async fn insert_cred(
    db: &TestDb,
    tenant: Uuid,
    provider: &str,
    key_hint: &str,
    status: &str,
    cooldown_until: Option<&str>,
) {
    let mut tx = tenant_tx(db, tenant).await;
    let cooldown: Option<chrono::DateTime<chrono::Utc>> =
        cooldown_until.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&chrono::Utc)));
    sqlx::query(
        "INSERT INTO provider_credentials \
            (tenant_id,provider,key_ciphertext,key_nonce,key_tag,dek_wrapped,kek_id,key_hint,status,cooldown_until) \
         VALUES ($1,$2,'\\x00','\\x00','\\x00','\\x00','kek-test',$3,$4::cred_status,$5)",
    )
    .bind(tenant)
    .bind(provider)
    .bind(key_hint)
    .bind(status)
    .bind(cooldown)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}
