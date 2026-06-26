// Regression: the boot/readiness migration check (`db::migrations_applied`) must recognize an
// applied migration whether the caller probes by the full file stem (`50001_M5_0001_sync_ops`,
// as main.rs's boot guard does), by a bare word (`users`, as /readyz does), or by sqlx's actual
// stored space-form description (`M5 0001 sync ops`). Before the fix the file-stem form never
// matched sqlx's de-prefixed/underscore->space description, so the server refused to boot (exit 3)
// against a fully migrated database. A non-existent migration must still be reported missing.
use sqlx::PgPool;

#[sqlx::test(migrations = "../migrations")]
async fn migrations_applied_recognizes_all_probe_forms(pool: PgPool) {
    // file-stem form — the exact argument main.rs's boot guard passes (the regressed case).
    assert!(
        app_server::db::migrations_applied(&pool, "50001_M5_0001_sync_ops").await,
        "file-stem probe must match the applied migration"
    );
    // bare-word form — what /readyz passes.
    assert!(
        app_server::db::migrations_applied(&pool, "users").await,
        "bare-word probe must match"
    );
    // sqlx's stored space-form description.
    assert!(
        app_server::db::migrations_applied(&pool, "M5 0001 sync ops").await,
        "space-form probe must match"
    );
    // a migration that was never applied must be reported missing.
    assert!(
        !app_server::db::migrations_applied(&pool, "99999_M9_9999_does_not_exist").await,
        "unknown migration must be reported missing"
    );
}
