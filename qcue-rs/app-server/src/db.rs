// QCue S3-R1/S3-R4 — qcue_app (non-bypass) + qcue_auth (narrow bootstrap) pools.
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// The canonical migration set, EMBEDDED at compile time from `migrations/`. Used by the boot guard to
/// verify the DB carries every migration this binary depends on (self-updating: a new migration file
/// tightens the check with zero code change — fixes the hardcoded-single-migration trap).
pub static MIGRATOR: Migrator = sqlx::migrate!("../migrations");

pub async fn app_pool(url: &str) -> sqlx::Result<PgPool> {
    PgPoolOptions::new().max_connections(16).connect(url).await
}
pub async fn auth_pool(url: &str) -> sqlx::Result<PgPool> {
    PgPoolOptions::new().max_connections(4).connect(url).await
}

/// The migration versions THIS binary was compiled with (from the embedded `MIGRATOR`).
pub fn required_migration_versions() -> Vec<i64> {
    MIGRATOR.iter().map(|m| m.version).collect()
}

/// The migration versions recorded as successfully applied in the DB's `_sqlx_migrations`.
pub async fn applied_migration_versions(pool: &PgPool) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar::<_, i64>(
        "SELECT version FROM _sqlx_migrations WHERE success ORDER BY version",
    )
    .fetch_all(pool)
    .await
}

/// Versions the binary requires that the DB is missing. Pure + testable; the boot guard refuses to
/// serve when this is non-empty (the partial-migration trap that crashed prod on a missing M6).
pub fn missing_migrations(required: &[i64], applied: &[i64]) -> Vec<i64> {
    let have: std::collections::HashSet<i64> = applied.iter().copied().collect();
    required.iter().copied().filter(|v| !have.contains(v)).collect()
}

/// /readyz dependency: are the expected migrations present?
pub async fn migrations_applied(pool: &PgPool, expected_latest: &str) -> bool {
    // The workspace migrations are applied via sqlx's migrator, which records each applied version in
    // `_sqlx_migrations` with a `description` derived from the file name: the numeric version prefix
    // is stripped and underscores become spaces (e.g. `50001_M5_0001_sync_ops.sql` is stored as
    // "M5 0001 sync ops"). Normalize the caller's probe — which may be a full file stem
    // (`50001_M5_0001_sync_ops`) or a bare word (`users`) — to that same form before matching, so the
    // readiness check is robust to the prefix scheme as the doc intends. Without this a file-stem
    // probe never matches and the server wrongly refuses to boot (exit 3) against a fully migrated DB.
    let needle = expected_latest
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '_')
        .replace('_', " ");
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM _sqlx_migrations WHERE description ILIKE $1",
    )
    .bind(format!("%{needle}%"))
    .fetch_one(pool)
    .await
    .map(|n| n > 0)
    .unwrap_or(false)
}
