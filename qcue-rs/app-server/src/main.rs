//! QCue S3 — boot: validate config (refuse-to-boot on a weak secret), build the two-role pools, assert
//! migrations are applied, build `AppState`, spawn the gated Auto-Dream scheduler cron, and serve on the
//! loopback default. The recall/wiki-query/dream/sync SSE surfaces + the CRDT sync hub are mounted by
//! `router::build_router`. Worker pools are gated (`*_ENABLED=false` in dev, pitfall #16).
use app_server::config::Config;
use app_server::ingest::RouterWikiLlm;
use app_server::objstore::ObjStore;
use app_server::state::AppState;
use app_server::vault::secrets::KmsSecrets;
use app_server::wire::hub::StreamHub;
use app_server::{db, dream, jobs, router};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    // refuse-to-boot on a weak/dev-default JWT secret or a missing DATABASE_URL/DATA_ROOT (S3-R14/R68).
    let cfg = match Config::from_env() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("config error: {e}");
            std::process::exit(2);
        }
    };
    let pool = db::app_pool(&cfg.database_url).await?;
    let auth_pool = db::auth_pool(&cfg.auth_database_url).await?;
    // Refuse to serve unless EVERY compiled-in migration is applied. Self-updating from the embedded
    // MIGRATOR, so a binary built ahead of the DB (the M6 prod incident: a wiki/sync write hitting a
    // not-yet-added column) is caught at boot, not at the first request. (Replaces the old single
    // hardcoded-migration probe, which only ever checked M5 and silently passed a DB missing M6.)
    let required = db::required_migration_versions();
    match db::applied_migration_versions(&pool).await {
        Ok(applied) => {
            let missing = db::missing_migrations(&required, &applied);
            if !missing.is_empty() {
                eprintln!(
                    "migrations not applied: DB is missing versions {missing:?}; \
                     run `sqlx migrate run --source migrations` before deploying this binary"
                );
                std::process::exit(3);
            }
        }
        Err(e) => {
            eprintln!("could not read _sqlx_migrations (is the DB initialized?): {e}");
            std::process::exit(3);
        }
    }

    // The KMS that wraps/unwraps the per-credential DEK. The vault's `KmsSecrets` seals/opens through it;
    // the dispatch `DbVaultResolver` decrypts BYOK keys through the SAME KMS. In production a real
    // `EnvKms` (master key from QCUE_KMS_KEY) protects the DEKs; when that env var is unset we fall back
    // to the INSECURE dev `StubKms` (public-constant KEK) and warn loudly. `EnvKms` also
    // decrypts legacy stub-wrapped DEKs, so existing BYOK keys keep working after the upgrade (S1-R38).
    let (kms, kek_id): (Arc<dyn secrets::Kms + Send + Sync>, &str) = match &cfg.kms_key {
        Some(master) => match secrets::EnvKms::from_bytes(master) {
            Some(env_kms) => (Arc::new(env_kms), "env-v1"),
            // validate() already rejects <32 bytes, so this arm is unreachable; fail closed anyway.
            None => {
                eprintln!("QCUE_KMS_KEY is invalid (must be >=32 bytes)");
                std::process::exit(2);
            }
        },
        None => {
            tracing::warn!(
                "QCUE_KMS_KEY is NOT set — the BYOK vault is using the INSECURE dev KMS (StubKms, a \
                 public-constant key). Stored provider keys are NOT confidential at rest. Set \
                 QCUE_KMS_KEY (>=32 random bytes, e.g. `openssl rand -base64 48`) in \
                 /etc/qcue/app-server.env for production."
            );
            (Arc::new(secrets::StubKms::new()), "stub-dev")
        }
    };
    let secrets = Arc::new(KmsSecrets::new(kms.clone(), kek_id));

    // The AGENTIC recall/wiki-query model seam: a router harness routing per (tenant, provider) that
    // ALSO executes the model-authored `recall_search` for real (RLS-scoped over the tenant's captures
    // under `<data_root>/objects`). Real dispatch when keys are configured; `QCUE_STUB_LLM=1` keeps it
    // keyless. The PLAIN `ingest_llm` (no tools) drives extraction so it never advertises recall tools.
    let vault_root_base = std::path::PathBuf::from(&cfg.data_root).join("objects");
    let recall_llm: Arc<dyn wiki::llm::WikiLlm> = Arc::new(RouterWikiLlm::live_recall(
        pool.clone(),
        kms.clone(),
        vault_root_base,
        "(recall unavailable)",
    ));
    let ingest_llm: Arc<dyn wiki::llm::WikiLlm> =
        Arc::new(RouterWikiLlm::live(pool.clone(), kms.clone(), "(ingest unavailable)"));
    // Voice transcription via the tenant's selected/auto-derived STT BYOK provider — D4 (multi-provider).
    let transcriber: Arc<dyn app_server::transcribe::Transcriber> =
        Arc::new(app_server::transcribe::RoutedTranscriber::new(pool.clone(), secrets.clone()));
    let state = AppState {
        cfg: cfg.clone(),
        pool,
        auth_pool,
        secrets,
        objstore: Arc::new(ObjStore::new(&cfg.data_root)),
        threads: StreamHub::new(),
        dream_streams: StreamHub::new(),
        recall_llm,
        ingest_llm,
        transcriber,
        jwks: Arc::new(app_server::auth::social::Jwks::new()),
    };

    // The per-tenant Auto-Dream scheduler cron — a no-op when DREAM_ENABLED is off (pitfall #16, S3-R54).
    dream::scheduler::spawn_scheduler(state.clone());

    // The ingest worker pool — claims `kind='ingest'` jobs (enqueued by POST /v1/capture) and extracts
    // each captured note into the wiki. A no-op when INGEST_WORKERS_ENABLED is off (pitfall #16). Without
    // this, captured notes persist to the tenant's `ideas` table but never become wiki pages.
    jobs::spawn::spawn_ingest_pool(state.clone());

    let app = router::build_router(state);
    let addr = format!("{}:{}", cfg.bind_addr, cfg.bind_port); // loopback default (S3-R70)
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "app-server listening");
    // Serve WITH per-connection info so the IP-keyed rate limiter (middleware::client_ip) sees the real
    // peer address instead of always falling back to 0.0.0.0 (which would collapse every client into one
    // shared bucket). Behind nginx the peer is the proxy, so QCUE_TRUSTED_PROXY must name it for the
    // X-Forwarded-For client IP to be honored (S3-R64).
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;
    Ok(())
}
