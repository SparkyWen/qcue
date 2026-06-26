//! QCue S5/§5.3 + §5.7 — the app-server dispatch wiring: the `DbVaultResolver` (DB+vault impl of the
//! router's `CredentialResolver`) plus the `build_harness` factory that the live model seams
//! (ingest `RouterWikiLlm`, recall, Dream) use to construct a `router::turn::Harness`.
//!
//! `build_harness` is the ONE place the server chooses stub-vs-real:
//!   - `QCUE_STUB_LLM=1` (or `=true`) → `Harness::with_stub` (keyless; demos/tests stay green).
//!   - otherwise → `Harness::with_dispatch(HttpDispatch)` with a `FallbackChain` built from the
//!     tenant's configured provider/model/api_mode (+ the profile's `fallback_models`).
//!
//! Provider/model are sourced from env (`QCUE_DEFAULT_PROVIDER` / `QCUE_DEFAULT_MODEL`) with safe
//! defaults; `api_mode` is derived from the registered provider profile (so Anthropic routes the
//! Messages wire, everyone else ChatCompletions). The per-tenant override (the `session_kv` model
//! picker) layers on top later without changing this seam.
pub mod price;
pub mod resolver;

pub use resolver::DbVaultResolver;

use protocol::{ApiMode, CanonicalUsage};
use router::dispatch_http::HttpDispatch;
use router::resolver::CredentialResolver;
use router::retry_loop::FallbackChain;
use router::turn::Harness;
use secrets::Kms;
use sqlx::PgPool;
use std::sync::Arc;
use store::cost_repo::{CostRepo, CostUsage};

/// Accrue one provider turn's usage into the TENANT cost ledger (best-effort — a ledger write must
/// NEVER fail the user's turn). Computes `cost_micros` from the model price table. This is the SINGLE
/// accrual site: every wiki/recall/dream provider call funnels through `RouterWikiLlm`, which calls
/// this after `run_turn`, so the ledger (which the audit found was structurally always 0) now grows
/// with real spend and `/v1/cost/today` + the pre-call ceiling become meaningful.
pub async fn accrue_turn_cost(
    pool: &PgPool,
    tenant: uuid::Uuid,
    provider: &str,
    model: &str,
    usage: &CanonicalUsage,
) {
    if *usage == CanonicalUsage::default() {
        return; // keyless stub / empty turn — nothing billed.
    }
    let cost = price::cost_micros(model, usage);
    let cu = CostUsage {
        input: usage.input as i64,
        output: usage.output as i64,
        cache_read: usage.cache_read as i64,
        cache_write: usage.cache_write as i64,
        reasoning: usage.reasoning as i64,
    };
    if let Err(e) = CostRepo::new(pool.clone()).accrue_tenant(tenant, cu, cost, provider).await {
        tracing::warn!(error = %e, %tenant, "cost ledger accrue failed (non-fatal)");
    }
}

/// Is the env-gated keyless stub selected? (`QCUE_STUB_LLM=1|true`). Demos/tests with no keys use this.
pub fn stub_llm_enabled() -> bool {
    std::env::var("QCUE_STUB_LLM").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
}

/// Is the live-internet recall tool (`web_fetch`/`web_search`) enabled? ON by default — the user-facing
/// assistant should be able to go online (Hermes-style: maximize capability). `QCUE_WEB_TOOLS=0|false|off`
/// is the operational kill-switch. The recall PROMPT (which advertises the tools) and the recall TOOL SET
/// (which wires the executor) both read this, so they never disagree.
pub fn web_tools_enabled() -> bool {
    match std::env::var("QCUE_WEB_TOOLS") {
        Ok(v) => !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "off" | "no"),
        Err(_) => true,
    }
}

/// The default route (provider, model) for a turn, from env with safe fallbacks (D7 first-class set).
fn default_route() -> (String, String) {
    let provider = std::env::var("QCUE_DEFAULT_PROVIDER").unwrap_or_else(|_| "openai".to_string());
    let model = std::env::var("QCUE_DEFAULT_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    (provider, model)
}

/// The selectable model catalog per first-class provider (D7). The ONE source of truth, shared by the
/// Settings model picker (`settings::routes`) and the per-tenant route resolver below. The FIRST entry
/// is the default model for a provider the tenant has a key for but hasn't explicitly picked.
pub fn provider_models(provider: &str) -> Vec<&'static str> {
    match provider {
        // CURATED to exactly two ids per the user preference: the NEWEST flagship + ONE low-price model.
        // gpt-5.5 is the newest bare chat id api.openai.com serves (verified live; `live_tool_calling.rs`
        // drives it end-to-end) and the default for a tenant who hasn't picked. gpt-5.4-mini is the single
        // low-price option. gpt-5.x require `max_completion_tokens` (handled in the ChatCompletions
        // transport). The fuller ladder (gpt-5.4/5.2/5.1/4o) was intentionally trimmed — a stale pick of a
        // now-unlisted id auto-heals to gpt-5.5 via `resolve_tenant_route`.
        "openai" => vec!["gpt-5.5", "gpt-5.4-mini"],
        // CURATED to two: the most capable GENERALLY-available flagship (opus-4-8, the user-named default;
        // verified live driving the full agentic recall loop) + ONE low-price model (haiku-4-5). opus-4-7
        // and sonnet-4-6 were trimmed per preference. claude-fable-5 stays OFF — it is access-gated, so a
        // normal BYOK key 404s ("Claude Fable 5 is not available. Please use Opus 4.8").
        "anthropic" => vec!["claude-opus-4-8", "claude-haiku-4-5"],
        // Already the curated pair: newest flagship + one low-price (flash).
        "gemini" => vec!["gemini-3-pro", "gemini-3-flash"],
        // Verified LIVE against api.deepseek.com (2026-06): the V4 generation is what `/models` now serves.
        // deepseek-v4-pro drives the full agentic recall loop end-to-end in the live test; both are
        // OpenAI-compatible and tool-call. (The legacy deepseek-chat/deepseek-reasoner ids still alias to V4
        // but are no longer listed by /models, so we surface the current generation; a stale pick of an
        // unlisted id auto-heals via `resolve_tenant_route`, which skips picks the catalog no longer offers.)
        "deepseek" => vec!["deepseek-v4-pro", "deepseek-v4-flash"],
        _ => vec![],
    }
}

/// RESP-R10 — is this (provider, model) ROUTABLE? True if the model is in the curated catalog OR matches
/// the provider's known family/capability. The curated `provider_models` stays the picker's DISPLAY list,
/// but routing/validation use THIS so a user-picked NEW variant (e.g. `gpt-5.5-pro`, a newer `claude-*`)
/// routes to ITSELF instead of being rejected (Settings 400) or silently rerouted to the default. Unknown
/// ids price at the conservative default; an unknown PROVIDER is not routable.
pub fn is_routable_model(provider: &str, model: &str) -> bool {
    if provider_models(provider).contains(&model) {
        return true;
    }
    let m = model.to_ascii_lowercase();
    match provider {
        // openai: the gpt-5 family, gpt-4*, or the o-series (an 'o' then a digit).
        "openai" => {
            m.starts_with("gpt-5")
                || m.starts_with("gpt-4")
                || (m.starts_with('o') && m.as_bytes().get(1).is_some_and(u8::is_ascii_digit))
        }
        "anthropic" => m.starts_with("claude-"),
        "gemini" => m.starts_with("gemini-"),
        "deepseek" => m.starts_with("deepseek-"),
        "kimi" => m.starts_with("kimi") || m.starts_with("moonshot"),
        "qwen" => m.starts_with("qwen"),
        _ => false,
    }
}

/// The human-facing display name for a provider id, used to tell the model (and the user) WHICH vendor it
/// truly runs on (identity transparency — see `ideas::recall::prompt::build_recall_prompt`). Unknown /
/// long-tail ids fall back to the id as-is. The stub harness reports a neutral label (never a real vendor).
pub fn provider_display_name(provider: &str) -> &str {
    match provider {
        "openai" => "OpenAI",
        "anthropic" => "Anthropic",
        "deepseek" => "DeepSeek",
        "gemini" => "Google Gemini",
        "kimi" => "Moonshot Kimi",
        "qwen" => "Alibaba Qwen",
        "openrouter" => "OpenRouter",
        "stub" => "the QCue stub harness",
        other => other,
    }
}

/// Resolve the effective (provider, model) for a turn from the tenant's BYOK config. The "active"
/// provider is the one whose key was added MOST RECENTLY (not alphabetical — that footgun routed a
/// DeepSeek user who also held a leftover Anthropic key to Claude, since `anthropic` < `deepseek`):
///   1. the most-recently-configured (primary) provider's OWN still-valid active-model pick (Settings
///      picker) — a stale pick for some OTHER provider can never hijack the primary;
///   2. else the primary provider's default (first catalog) model (falling through to the next
///      most-recent provider only if the primary has no known catalog default, e.g. a custom one);
///   3. else the env-default route — so keyless/unconfigured tenants and demos behave as before.
///
/// All reads are RLS-bound in one tx; any DB error degrades to the env default (never blocks a turn).
pub async fn effective_route(pool: &PgPool, tenant: uuid::Uuid) -> (String, String) {
    let (provider, model, _) = effective_route_with_providers(pool, tenant).await;
    (provider, model)
}

/// Like [`effective_route`] but also returns the tenant's full provider list (most-recently-added first),
/// so the caller can build a CROSS-PROVIDER fallback chain (F-12). Degrades to the env default + an empty
/// provider list on no-config / DB error (never blocks a turn).
pub async fn effective_route_with_providers(
    pool: &PgPool,
    tenant: uuid::Uuid,
) -> (String, String, Vec<String>) {
    match resolve_tenant_route(pool, tenant).await {
        Ok(Some(route)) => route,
        Ok(None) | Err(_) => {
            let (provider, model) = default_route();
            (provider, model, Vec::new())
        }
    }
}

async fn resolve_tenant_route(
    pool: &PgPool,
    tenant: uuid::Uuid,
) -> Result<Option<(String, String, Vec<String>)>, sqlx::Error> {
    use sqlx::Row;
    let mut tx = pool.begin().await?;
    // FORCE RLS on provider_credentials/session_kv → bind the GUC per tx (matches DbVaultResolver).
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await?;
    // Providers the tenant has at least one key for, MOST-RECENTLY-ADDED FIRST. The first entry is the
    // "primary" (active) provider; `provider` is the deterministic tie-break when timestamps collide.
    let provider_rows = sqlx::query(
        "SELECT provider FROM provider_credentials GROUP BY provider ORDER BY MAX(created_at) DESC, provider",
    )
    .fetch_all(&mut *tx)
    .await?;
    let providers: Vec<String> = provider_rows.iter().map(|r| r.get::<String, _>("provider")).collect();
    let Some(primary) = providers.first() else {
        tx.commit().await?;
        return Ok(None);
    };
    // (1) Honor ONLY the primary provider's explicit, still-valid active-model pick. The pick lives in
    //     session_kv under the settings session (Uuid::nil), key `model:<provider>`. Restricting to the
    //     primary means a stale `model:<other>` pick can never hijack the most-recently-configured route.
    let key = format!("model:{primary}");
    let picked: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT value FROM session_kv WHERE session_id = $1 AND key = $2")
            .bind(uuid::Uuid::nil())
            .bind(&key)
            .fetch_optional(&mut *tx)
            .await?;
    let model = picked.and_then(|v| v.get("model").and_then(|m| m.as_str()).map(str::to_string));
    if let Some(model) = model {
        // RESP-R10 — honor a stored pick the harness can route (catalog OR a known family variant), so a
        // newer gpt-5.x/claude variant the user picked isn't silently dropped back to the default.
        if is_routable_model(primary, &model) {
            tx.commit().await?;
            return Ok(Some((primary.clone(), model, providers.clone())));
        }
    }
    // (2) No valid pick → the first provider (most-recent order) with a known default catalog model.
    for p in &providers {
        if let Some(default_model) = provider_models(p).first() {
            tx.commit().await?;
            return Ok(Some((p.clone(), (*default_model).to_string(), providers.clone())));
        }
    }
    tx.commit().await?;
    Ok(None)
}

/// The (provider, model, api_mode) fallback links for a turn: the primary, then the primary profile's
/// same-provider `fallback_models`, then — F-12 — a CROSS-PROVIDER link for each OTHER provider the tenant
/// has a key for (`extra_providers`), each with that provider's default catalog model and its OWN
/// re-derived `api_mode` (so an Anthropic hop routes the Messages wire — pitfall #20). Pure + unit-testable;
/// `extra_providers` may safely include the primary (it is skipped) and unknown/catalog-less providers
/// (skipped — we never add a link we can't route or price).
fn build_chain_links(
    registry: &providers::registry::Registry,
    primary_provider: &str,
    primary_model: &str,
    extra_providers: &[String],
) -> Vec<(String, String, ApiMode)> {
    // RESP-R2 — the wire is per-(provider, MODEL): an openai gpt-5.x link routes Responses while an
    // openai gpt-4o link routes chat. So each link re-derives its api_mode from its OWN model.
    let api_mode_of = |p: &str, m: &str| {
        registry.get(p).map(|pr| providers::effective_api_mode(pr, m)).unwrap_or(ApiMode::ChatCompletions)
    };
    let mut links: Vec<(String, String, ApiMode)> = vec![(
        primary_provider.to_string(),
        primary_model.to_string(),
        api_mode_of(primary_provider, primary_model),
    )];
    // same-provider fallback models (the prior behavior).
    if let Some(p) = registry.get(primary_provider) {
        for fm in &p.fallback_models {
            if fm != primary_model {
                links.push((primary_provider.to_string(), fm.clone(), api_mode_of(primary_provider, fm)));
            }
        }
    }
    // F-12 — cross-provider links: each OTHER provider the tenant has a key for, with its default catalog
    // model and its OWN re-derived api_mode. Skip the primary (already chained) and any provider with no
    // known default model (we must not add a link the dispatch can't route or the ledger can't price).
    for p in extra_providers {
        if p == primary_provider {
            continue;
        }
        if let Some(default_model) = provider_models(p).first() {
            let link = (p.to_string(), (*default_model).to_string(), api_mode_of(p, default_model));
            if !links.iter().any(|l| l.0 == link.0 && l.1 == link.1) {
                links.push(link);
            }
        }
    }
    links
}

/// Build the `FallbackChain` rooted at (provider, model) with the tenant's other providers as
/// cross-provider fallbacks (`extra_providers`; empty for the env-default path).
fn build_chain(
    registry: &providers::registry::Registry,
    provider: &str,
    model: &str,
    extra_providers: &[String],
) -> FallbackChain {
    FallbackChain::new(build_chain_links(registry, provider, model, extra_providers))
}

/// Construct the per-turn `Harness` for `tenant`. The stub fallback keeps every keyless path working;
/// the real path wires `HttpDispatch` (transport_for(api_mode) + the DB-backed pool/resolver + the
/// classify/rotate/fallback/backoff recovery loop) over the registered provider profiles.
pub fn build_harness(pool: PgPool, kms: Arc<dyn Kms + Send + Sync>, stub_reply: &str) -> Harness {
    if stub_llm_enabled() {
        return stub_harness(stub_reply);
    }
    let (provider, model) = default_route();
    build_live_harness(pool, kms, &provider, &model, &[])
}

/// Construct the per-turn `Harness` for `tenant`, rooting the `FallbackChain` at the provider/model the
/// tenant actually configured (`effective_route`) instead of the env default. This is the seam that
/// fixes recall/ingest/dream for a BYOK tenant: a deepseek-only tenant now routes to deepseek, not the
/// openai env default. Async because it reads the tenant's BYOK config. `QCUE_STUB_LLM=1` stays keyless.
/// Returns the harness AND the resolved `(provider, model)` it routes to, so the caller can price the
/// turn's usage for the cost ledger. The stub path reports `("stub","stub")` (never billed).
pub async fn build_harness_for(
    pool: PgPool,
    kms: Arc<dyn Kms + Send + Sync>,
    tenant: uuid::Uuid,
    stub_reply: &str,
) -> (Harness, String, String) {
    if stub_llm_enabled() {
        return (stub_harness(stub_reply), "stub".to_string(), "stub".to_string());
    }
    let (provider, model, others) = effective_route_with_providers(&pool, tenant).await;
    // F-12 — the tenant's OTHER providers become cross-provider fallback links (a deepseek-primary tenant
    // that also holds openai/anthropic keys now fails over across vendors, not just across deepseek models).
    let harness = build_live_harness(pool, kms, &provider, &model, &others);
    (harness, provider, model)
}

/// Parse a recall effort wire token (`minimal|low|medium|high|xhigh|max`, case-insensitive) into the
/// typed `providers::hooks::Effort`. Unknown/empty → `None` (the turn uses the provider default). The
/// tokens mirror the Dart `RecallEffort.wire` values (v0.2.2 recall model/effort picker).
pub fn parse_effort(s: &str) -> Option<providers::hooks::Effort> {
    use providers::hooks::Effort;
    match s.trim().to_ascii_lowercase().as_str() {
        "minimal" => Some(Effort::Minimal),
        "low" => Some(Effort::Low),
        "medium" => Some(Effort::Medium),
        "high" => Some(Effort::High),
        "xhigh" => Some(Effort::XHigh),
        "max" => Some(Effort::Max),
        _ => None,
    }
}

/// Build the per-turn `Harness` rooted at an EXPLICIT (provider, model) override (the recall picker)
/// instead of the tenant's default active model. Still uses the tenant's BYOK resolver, and appends the
/// tenant's OTHER configured providers as cross-provider fallback (resilience). `QCUE_STUB_LLM=1` stays
/// keyless. Returns the harness AND the (provider, model) it routes to (for cost accrual + identity).
/// The override is only ever a provider the tenant has a key for (the app picker offers only those), so
/// credential/RLS isolation is unchanged; an un-keyed provider simply fails the resolver and falls back.
pub async fn build_harness_for_route(
    pool: PgPool,
    kms: Arc<dyn Kms + Send + Sync>,
    tenant: uuid::Uuid,
    provider: &str,
    model: &str,
    stub_reply: &str,
) -> (Harness, String, String) {
    if stub_llm_enabled() {
        return (stub_harness(stub_reply), "stub".to_string(), "stub".to_string());
    }
    // The tenant's other providers stay available as cross-provider fallback links (F-12).
    let (_, _, others) = effective_route_with_providers(&pool, tenant).await;
    let harness = build_live_harness(pool, kms, provider, model, &others);
    (harness, provider.to_string(), model.to_string())
}

/// The keyless scripted stub harness (demos/tests/`QCUE_STUB_LLM=1`).
fn stub_harness(stub_reply: &str) -> Harness {
    Harness::with_stub(router::stub::StubProvider::new(router::stub::StubScript::text(stub_reply)))
}

/// Wire the live `HttpDispatch` (registry + DB/vault resolver + a chain rooted at provider/model).
fn build_live_harness(
    pool: PgPool,
    kms: Arc<dyn Kms + Send + Sync>,
    provider: &str,
    model: &str,
    extra_providers: &[String],
) -> Harness {
    let registry = Arc::new(providers::registry::register_all());
    let resolver: Arc<dyn CredentialResolver> = Arc::new(DbVaultResolver::new(pool, kms));
    let chain = build_chain(&registry, provider, model, extra_providers);
    let (opts, allow_insecure) = http::client::opts_from_env();
    // A client build failure (TLS init) is fatal for the real path; fall back to a default client.
    let client = http::client::build_client(opts).unwrap_or_default();
    let dispatch = HttpDispatch::new(client, registry, resolver, chain, allow_insecure);
    Harness::with_dispatch(Box::new(dispatch))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn chain_appends_cross_provider_links_with_their_own_api_mode() {
        // F-12 — a deepseek-primary tenant that also holds openai + anthropic keys must fail over ACROSS
        // vendors, each cross link re-deriving its own wire (Anthropic → Messages; pitfall #20).
        let reg = providers::registry::register_all();
        let links = build_chain_links(
            &reg,
            "deepseek",
            "deepseek-chat",
            &["openai".to_string(), "anthropic".to_string()],
        );
        let provs: Vec<&str> = links.iter().map(|(p, _, _)| p.as_str()).collect();
        assert_eq!(provs.first(), Some(&"deepseek"), "primary stays first: {provs:?}");
        assert!(provs.contains(&"openai"), "cross-provider openai must be appended: {provs:?}");
        assert!(provs.contains(&"anthropic"), "cross-provider anthropic must be appended: {provs:?}");
        let anth = links.iter().find(|(p, _, _)| p == "anthropic").unwrap();
        assert_eq!(anth.2, ApiMode::AnthropicMessages, "anthropic hop re-derives the Messages wire");
        let oai = links.iter().find(|(p, _, _)| p == "openai").unwrap();
        assert_eq!(oai.2, ApiMode::Responses, "openai default (gpt-5.5) routes the Responses wire (RESP-R2)");
        assert_eq!(oai.1, provider_models("openai")[0], "cross link uses the provider's default model");
    }

    #[test]
    fn routable_accepts_catalog_and_family_variants() {
        // RESP-R10 — catalog ids AND known-family variants route; foreign ids don't.
        assert!(is_routable_model("openai", "gpt-5.5"), "catalog id");
        assert!(is_routable_model("openai", "gpt-5.5-pro"), "NEW gpt-5.x variant must route to itself");
        assert!(is_routable_model("openai", "gpt-4o"));
        assert!(is_routable_model("openai", "o3"));
        assert!(is_routable_model("anthropic", "claude-opus-4-9"), "newer claude variant");
        assert!(is_routable_model("deepseek", "deepseek-v5-pro"));
        assert!(!is_routable_model("openai", "totally-made-up"));
        assert!(!is_routable_model("no-such-provider", "gpt-5.5"));
    }

    #[test]
    fn chain_link_api_mode_is_model_aware() {
        // RESP-R2/R11 — a gpt-5.5 link routes Responses; the refreshed same-provider fallback (gpt-5.4-mini)
        // is ALSO gpt-5.x → Responses.
        let reg = providers::registry::register_all();
        let links = build_chain_links(&reg, "openai", "gpt-5.5", &[]);
        let primary = links.iter().find(|(p, m, _)| p == "openai" && m == "gpt-5.5").unwrap();
        assert_eq!(primary.2, ApiMode::Responses, "gpt-5.5 link routes Responses: {links:?}");
        assert!(
            links.iter().any(|(p, m, am)| p == "openai" && m == "gpt-5.4-mini" && *am == ApiMode::Responses),
            "fallback refreshed to gpt-5.4-mini + Responses: {links:?}",
        );
        // a gpt-4o primary stays on chat (per-model, not per-provider).
        let chat_links = build_chain_links(&reg, "openai", "gpt-4o", &[]);
        assert_eq!(chat_links[0].2, ApiMode::ChatCompletions, "gpt-4o link stays chat: {chat_links:?}");
    }

    #[test]
    fn parse_effort_maps_wire_tokens_and_rejects_garbage() {
        use providers::hooks::Effort;
        assert_eq!(parse_effort("minimal"), Some(Effort::Minimal));
        assert_eq!(parse_effort("low"), Some(Effort::Low));
        assert_eq!(parse_effort("medium"), Some(Effort::Medium));
        assert_eq!(parse_effort("high"), Some(Effort::High));
        assert_eq!(parse_effort("xhigh"), Some(Effort::XHigh));
        assert_eq!(parse_effort("XHIGH"), Some(Effort::XHigh)); // case-insensitive
        assert_eq!(parse_effort(" max "), Some(Effort::Max)); // trimmed
        assert_eq!(parse_effort("turbo"), None); // unknown → provider default
        assert_eq!(parse_effort(""), None);
    }

    #[test]
    fn provider_display_names_are_human_facing_and_fall_back() {
        assert_eq!(provider_display_name("deepseek"), "DeepSeek");
        assert_eq!(provider_display_name("openai"), "OpenAI");
        assert_eq!(provider_display_name("anthropic"), "Anthropic");
        // unknown / long-tail id passes through unchanged.
        assert_eq!(provider_display_name("my-custom-vendor"), "my-custom-vendor");
        // stub never names a real vendor.
        assert_eq!(provider_display_name("stub"), "the QCue stub harness");
    }

    #[test]
    fn chain_skips_the_primary_in_extras_and_unknown_providers() {
        let reg = providers::registry::register_all();
        // `openai` is both primary and in extras → must not be re-added as a default-model link; an
        // unknown provider with no catalog is skipped (never a link we can't route).
        let links = build_chain_links(
            &reg,
            "openai",
            "gpt-4o",
            &["openai".to_string(), "totally-unknown".to_string(), "deepseek".to_string()],
        );
        let provs: Vec<&str> = links.iter().map(|(p, _, _)| p.as_str()).collect();
        assert!(provs.contains(&"deepseek"), "a real extra is appended: {provs:?}");
        assert!(!provs.contains(&"totally-unknown"), "catalog-less provider is skipped: {provs:?}");
        assert!(
            !links.iter().any(|(p, m, _)| p == "openai" && m == provider_models("openai")[0]),
            "the extra `openai` must not re-add the default-model link (primary already covers it): {provs:?}",
        );
    }
}
