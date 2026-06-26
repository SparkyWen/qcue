// QCue — per-tenant provider/model resolution for a turn (the recall "no credentials for openai" bug).
// A tenant who configured ONLY a DeepSeek BYOK key must route to deepseek (with a real, tool-capable
// model id), NOT the env-default openai. These pin `effective_route` + the corrected model catalog.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::dispatch::{effective_route, provider_models};
use sqlx::PgPool;
use uuid::Uuid;

/// Set a tenant's active-model pick directly in `session_kv` (mirrors the Settings PUT, but lets a test
/// seed a value the live catalog might not contain — for the stale-pick case).
async fn set_active_model(db: &TestDb, tenant: Uuid, provider: &str, model: &str) {
    let mut tx = tenant_tx(db, tenant).await;
    let key = format!("model:{provider}");
    let value = serde_json::json!({ "model": model });
    sqlx::query("INSERT INTO session_kv (tenant_id, session_id, key, value) VALUES ($1,$2,$3,$4)")
        .bind(tenant)
        .bind(Uuid::nil())
        .bind(&key)
        .bind(&value)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

/// Backdate a provider's credential `created_at` so "most-recently-added" ordering is deterministic
/// (insert_cred stamps now(); two sequential inserts can land in the same microsecond).
async fn backdate_cred(db: &TestDb, tenant: Uuid, provider: &str, when: &str) {
    let mut tx = tenant_tx(db, tenant).await;
    sqlx::query("UPDATE provider_credentials SET created_at = $1::timestamptz WHERE provider = $2")
        .bind(when)
        .bind(provider)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

#[sqlx::test(migrations = "../migrations")]
async fn effective_route_prefers_most_recently_added_key_over_alphabetical(pool: PgPool) {
    // The footgun: ordering providers alphabetically routes a tenant who ALSO holds an older
    // anthropic key to anthropic (a<d), so a "DeepSeek user" silently chats with Claude. The
    // active provider must be the one whose key was added MOST RECENTLY, not the alphabetical first.
    let db = from_pool(pool);
    let (tid, _uid) = seed_tenant(&db, "route-recency").await;
    insert_cred(&db, tid, "anthropic", "an-old", "ok", None).await;
    backdate_cred(&db, tid, "anthropic", "2020-01-01T00:00:00Z").await;
    insert_cred(&db, tid, "deepseek", "ds-new", "ok", None).await;

    let (provider, model) = effective_route(&db.app, tid).await;
    assert_eq!(provider, "deepseek", "most-recently-added key (deepseek) must win over alphabetical anthropic");
    assert!(provider_models("deepseek").contains(&model.as_str()), "got {model}");
}

#[sqlx::test(migrations = "../migrations")]
async fn effective_route_ignores_a_stale_pick_from_a_non_primary_provider(pool: PgPool) {
    // A leftover `model:anthropic` pick from prior testing must NOT hijack a tenant whose most
    // recently configured provider is deepseek and who has no deepseek pick.
    let db = from_pool(pool);
    let (tid, _uid) = seed_tenant(&db, "route-nonprimary-pick").await;
    insert_cred(&db, tid, "anthropic", "an-old", "ok", None).await;
    backdate_cred(&db, tid, "anthropic", "2020-01-01T00:00:00Z").await;
    set_active_model(&db, tid, "anthropic", "claude-opus-4-8").await;
    insert_cred(&db, tid, "deepseek", "ds-new", "ok", None).await;

    let (provider, _model) = effective_route(&db.app, tid).await;
    assert_eq!(provider, "deepseek", "a stale non-primary anthropic pick must not override the deepseek primary");
}

#[test]
fn deepseek_catalog_uses_real_tool_capable_model_ids() {
    let m = provider_models("deepseek");
    // The V4 generation is what api.deepseek.com/models serves today; both are OpenAI-compatible and
    // tool-call (verified live — deepseek-v4-pro drives the full agentic recall loop end-to-end).
    assert!(m.contains(&"deepseek-v4-pro"), "deepseek-v4-pro must be offered: {m:?}");
    assert!(m.contains(&"deepseek-v4-flash"), "deepseek-v4-flash must be offered: {m:?}");
    // the legacy deepseek-chat/deepseek-reasoner ids are no longer LISTED by /models (they alias to V4),
    // so the catalog surfaces the current generation; a stale pick of a delisted id auto-heals on resolve.
    assert!(!m.contains(&"deepseek-reasoner"), "the delisted deepseek-reasoner must be gone: {m:?}");
}

#[test]
fn openai_catalog_has_no_phantom_models() {
    let m = provider_models("openai");
    // CURATED to exactly two: the newest flagship (default, newest-first) + one low-price model.
    assert_eq!(m, vec!["gpt-5.5", "gpt-5.4-mini"], "openai catalog is the curated pair: {m:?}");
    assert_eq!(m.first(), Some(&"gpt-5.5"), "gpt-5.5 must be the default (newest-first): {m:?}");
    assert!(m.contains(&"gpt-5.4-mini"), "the single low-price gpt-5.4-mini must be offered: {m:?}");
    // these ids 404 from api.openai.com and must never be advertised.
    assert!(!m.contains(&"gpt-5.1-mini"), "phantom gpt-5.1-mini must be gone: {m:?}");
    assert!(!m.contains(&"o4-mini"), "dead o4-mini must be gone: {m:?}");
    assert!(!m.contains(&"o4"), "phantom o4 must be gone: {m:?}");
    assert!(!m.is_empty());
}

#[sqlx::test(migrations = "../migrations")]
async fn effective_route_honors_a_valid_active_model_pick(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, _uid) = seed_tenant(&db, "route-pick").await;
    insert_cred(&db, tid, "deepseek", "ds-1234", "ok", None).await;
    set_active_model(&db, tid, "deepseek", "deepseek-v4-flash").await;

    let (provider, model) = effective_route(&db.app, tid).await;
    assert_eq!(provider, "deepseek");
    assert_eq!(model, "deepseek-v4-flash", "a valid, currently-offered pick is honored");
}

#[sqlx::test(migrations = "../migrations")]
async fn effective_route_honors_a_known_family_variant_pick(pool: PgPool) {
    // RESP-R10 — a pick that is NOT a literal catalog entry but IS a known family/capability variant (e.g.
    // a newer deepseek-v* the curated list hasn't caught up to) routes to ITSELF, instead of being silently
    // healed back to the default. New variants must not be dropped (a stale family id that 404s upstream is
    // caught by the cross-model fallback chain, so routing it degrades gracefully).
    let db = from_pool(pool);
    let (tid, _uid) = seed_tenant(&db, "route-family-variant").await;
    insert_cred(&db, tid, "deepseek", "ds-1234", "ok", None).await;
    set_active_model(&db, tid, "deepseek", "deepseek-v5-pro").await; // a future family variant, not yet in catalog

    let (provider, model) = effective_route(&db.app, tid).await;
    assert_eq!(provider, "deepseek");
    assert_eq!(model, "deepseek-v5-pro", "a known-family variant pick routes to itself (RESP-R10)");
}

#[sqlx::test(migrations = "../migrations")]
async fn effective_route_uses_the_provider_with_a_key_when_no_pick(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, _uid) = seed_tenant(&db, "route-keyonly").await;
    insert_cred(&db, tid, "deepseek", "ds-1234", "ok", None).await;

    let (provider, model) = effective_route(&db.app, tid).await;
    assert_eq!(provider, "deepseek", "a deepseek-only tenant must NOT route to openai");
    assert!(
        provider_models("deepseek").contains(&model.as_str()),
        "the default model must be a valid catalog model, got {model}"
    );
}

#[sqlx::test(migrations = "../migrations")]
async fn effective_route_falls_back_to_env_default_when_no_keys(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, _uid) = seed_tenant(&db, "route-nokeys").await;

    // No credentials configured → keep the env-default behavior (demos/keyless tenants).
    let (provider, model) = effective_route(&db.app, tid).await;
    assert_eq!(provider, "openai");
    assert_eq!(model, "gpt-4o-mini");
}

#[sqlx::test(migrations = "../migrations")]
async fn effective_route_heals_an_unroutable_pick(pool: PgPool) {
    // RESP-R10 — a pick that is neither in the catalog NOR a known family variant (a truly foreign/garbage
    // id, or one for the wrong vendor) must HEAL to the provider default rather than route to something that
    // can't exist. The family check is the line between "new variant, route it" and "unroutable, heal it".
    let db = from_pool(pool);
    let (tid, _uid) = seed_tenant(&db, "route-unroutable").await;
    insert_cred(&db, tid, "deepseek", "ds-1234", "ok", None).await;
    set_active_model(&db, tid, "deepseek", "totally-foreign-model-9000").await;

    let (provider, model) = effective_route(&db.app, tid).await;
    assert_eq!(provider, "deepseek");
    assert_ne!(model, "totally-foreign-model-9000", "an unroutable pick must not be used");
    assert!(provider_models("deepseek").contains(&model.as_str()), "heals to a catalog default");
}
