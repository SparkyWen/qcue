// QCue — the boot migration guard must catch a DB missing ANY compiled-in migration (the M6 prod
// incident: binary deployed ahead of `sqlx migrate run`, server booted, then crashed on a missing
// wiki_pages.content_hash column). These pin the self-updating check.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use app_server::db::{
    applied_migration_versions, missing_migrations, required_migration_versions,
};
use sqlx::PgPool;

#[test]
fn missing_migrations_flags_an_unapplied_version() {
    let required = vec![1, 2, 50001, 60001];
    // DB migrated only through M5 (50001) — M6 (60001) is missing, exactly the prod incident.
    let applied = vec![1, 2, 50001];
    assert_eq!(missing_migrations(&required, &applied), vec![60001]);
}

#[test]
fn missing_migrations_empty_when_fully_applied() {
    let required = vec![1, 2, 50001, 60001];
    let applied = vec![60001, 50001, 2, 1]; // order-independent
    assert!(missing_migrations(&required, &applied).is_empty());
}

#[test]
fn required_versions_include_m6_content_hash_migration() {
    // The embedded MIGRATOR must know about 60001 (the migration that adds content_hash/sync_version/
    // sync_ops.seq) — otherwise the guard could never catch a DB missing it.
    assert!(required_migration_versions().contains(&60001), "MIGRATOR must embed M6 (60001)");
}

#[sqlx::test(migrations = "../migrations")]
async fn a_fully_migrated_db_has_no_missing_migrations(pool: PgPool) {
    // #[sqlx::test] applies ALL migrations to the ephemeral DB → the boot guard must pass (empty).
    let required = required_migration_versions();
    let applied = applied_migration_versions(&pool).await.unwrap();
    let missing = missing_migrations(&required, &applied);
    assert!(missing.is_empty(), "fully-migrated DB must report no missing migrations, got {missing:?}");
}
