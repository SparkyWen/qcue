// QCue S1-R35 — the DbVaultResolver write-back: a credential that just SUCCEEDED heals.
//
// Tasks A1 (TTL+cap) and A2 (Retry-After) bounded how LONG a cred stays cooled; A3 closes the loop so a
// cooled cred clears the moment it next succeeds, instead of staying `exhausted` until the user re-saves
// the key in Settings (the real cause of the operator's "stuck for 8 hours" report). This pins the
// persistence half: marking the in-memory pool Ok and persisting clears `status`/`cooldown_until` for a
// row that was `exhausted`. (The dispatch_http success-path wiring is covered indirectly — it just calls
// `mark_ok` + this `persist_transitions`.)
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::dispatch::DbVaultResolver;
use router::resolver::CredentialResolver; // the trait that owns pool_for/persist_transitions
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

/// Read a credential's persisted (status, cooldown_until) under the tenant GUC (FORCE RLS).
async fn cred_state(
    db: &TestDb,
    tenant: Uuid,
    key_hint: &str,
) -> (String, Option<chrono::DateTime<chrono::Utc>>) {
    let mut tx = tenant_tx(db, tenant).await;
    let row = sqlx::query(
        "SELECT status::text AS s, cooldown_until FROM provider_credentials WHERE key_hint = $1",
    )
    .bind(key_hint)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (row.get::<String, _>("s"), row.try_get("cooldown_until").unwrap())
}

/// Build the real DB+vault resolver over the test pool with the keyless StubKms.
fn resolver(pool: PgPool) -> DbVaultResolver {
    DbVaultResolver::new(pool, Arc::new(secrets::StubKms::new()))
}

#[sqlx::test(migrations = "../migrations")]
async fn test_persist_transitions_clears_cooldown_on_ok(pool: PgPool) {
    // Seed a tenant + an EXHAUSTED openai credential with a cooldown an hour in the future.
    let db = from_pool(pool.clone());
    let (tid, _uid) = seed_tenant(&db, "heal").await;
    let cooldown = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    insert_cred(&db, tid, "openai", "sk-hint", "exhausted", Some(&cooldown)).await;

    // sanity: it really starts cooled.
    let (s0, c0) = cred_state(&db, tid, "sk-hint").await;
    assert_eq!(s0, "exhausted", "precondition: the seeded cred is exhausted");
    assert!(c0.is_some(), "precondition: a cooldown is set");

    // Load the pool, heal the cred in-memory (a success would call this), then persist.
    let r = resolver(pool);
    let mut cp = r.pool_for(tid, "openai").await.unwrap();
    cp.mark_ok("sk-hint");
    r.persist_transitions(tid, "openai", &cp).await.unwrap();

    // The row is healed: status ok, cooldown cleared.
    let (s, c) = cred_state(&db, tid, "sk-hint").await;
    assert_eq!(s, "ok", "a successful call heals the row back to ok");
    assert!(c.is_none(), "the cooldown is cleared (was stuck until a key re-save)");
}

#[sqlx::test(migrations = "../migrations")]
async fn test_persist_transitions_leaves_an_already_ok_row_untouched(pool: PgPool) {
    // An Ok cred that stays Ok must not be churned by the heal UPDATE (the clear only targets
    // still-`exhausted` rows). Proves the Ok-arm is a no-op for an already-healthy cred.
    let db = from_pool(pool.clone());
    let (tid, _uid) = seed_tenant(&db, "heal-noop").await;
    insert_cred(&db, tid, "openai", "sk-ok", "ok", None).await;

    let r = resolver(pool);
    let mut cp = r.pool_for(tid, "openai").await.unwrap();
    cp.mark_ok("sk-ok");
    r.persist_transitions(tid, "openai", &cp).await.unwrap();

    let (s, c) = cred_state(&db, tid, "sk-ok").await;
    assert_eq!(s, "ok");
    assert!(c.is_none());
}
