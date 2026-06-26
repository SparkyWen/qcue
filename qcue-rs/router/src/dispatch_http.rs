// QCue S5/§5.2 — HttpDispatch: the real provider HTTP bridge. The ONE place a live LLM call is made.
//
// `complete()` runs the per-call Hermes recovery loop, reusing the already-built primitives:
//   transport_for(api_mode) · CredentialPool · apply_anthropic_cache_control · the `http` crate
//   client+versioned_base_url · classify · decide_action (Rotate/Fallback/Backoff/Abort).
// It NEVER branches on provider name; api_mode is reached only through `transport_for`.
use crate::classify::{classify, ClassifyCtx};
use crate::dispatch::{DispatchRequest, ProviderDispatch};
use crate::prompt_cache::apply_anthropic_cache_control_to_body;
use crate::resolver::CredentialResolver;
use crate::retry_loop::{decide_action, Action, FallbackChain};
use crate::transport::transport_for;
use async_trait::async_trait;
use protocol::{ApiError, ApiMode, FailoverReason, Message, NormalizedResponse};
use providers::profile::ProviderProfile;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct HttpDispatch {
    pub client: reqwest::Client,
    pub registry: Arc<providers::registry::Registry>,
    pub resolver: Arc<dyn CredentialResolver>,
    /// (provider, model, api_mode) links for this turn; Mutex because `complete` takes `&self`.
    pub chain: std::sync::Mutex<FallbackChain>,
    /// `QCUE_ALLOW_INSECURE_HTTP` — only loopback http:// is allowed unless this is set.
    pub allow_insecure: bool,
}

impl HttpDispatch {
    pub fn new(
        client: reqwest::Client,
        registry: Arc<providers::registry::Registry>,
        resolver: Arc<dyn CredentialResolver>,
        chain: FallbackChain,
        allow_insecure: bool,
    ) -> Self {
        Self { client, registry, resolver, chain: std::sync::Mutex::new(chain), allow_insecure }
    }

    fn current_link(&self) -> (String, String, ApiMode) {
        let g = self.chain.lock().unwrap_or_else(|e| e.into_inner());
        g.current().clone()
    }

    /// Advance the fallback chain. Returns false when no further provider remains.
    fn advance_chain(&self) -> bool {
        let mut g = self.chain.lock().unwrap_or_else(|e| e.into_inner());
        g.advance().is_some()
    }
}

/// S1-R15 — the wire path for each api_mode (joined to the profile's versioned base_url).
fn endpoint_for(api_mode: ApiMode) -> &'static str {
    match api_mode {
        ApiMode::ChatCompletions => "chat/completions",
        ApiMode::AnthropicMessages => "messages",
        ApiMode::Responses => "responses", // D19 — POST {base}/responses
    }
}

/// A static fallback profile for an unregistered provider (generic ChatCompletions, no cache).
fn fallback_profile(provider: &str) -> ProviderProfile {
    use providers::hooks::DefaultHooks;
    use providers::profile::{AuthType, TempPolicy};
    ProviderProfile {
        name: provider.to_string(),
        api_mode: ApiMode::ChatCompletions,
        base_url: String::new(),
        models_url: None,
        auth_type: AuthType::ApiKey,
        default_headers: Default::default(),
        env_http_headers: Default::default(),
        fixed_temperature: TempPolicy::Inherit,
        default_max_tokens: Some(4096),
        fallback_models: vec![],
        supports_vision: false,
        request_max_retries: 3,
        stream_idle_timeout_ms: 30_000,
        stream_ttfb_timeout_ms: 60_000,
        cache_supported: false,
        hooks: Box::new(DefaultHooks),
    }
}

/// Build the outbound HTTP request body for this api_mode, applying Anthropic cache_control when the
/// provider supports it. Pure (no I/O) so the wiremock/unit tests can assert the exact shape.
pub fn build_outbound_body(
    profile: &ProviderProfile,
    model: &str,
    messages: &[Message],
    tools: &[protocol::ToolDef],
    params: &crate::transport::ReqParams,
) -> serde_json::Value {
    // RESP-R2 — resolve the wire PER MODEL (OpenAI gpt-5.x → Responses), not just per provider.
    let api_mode = providers::effective_api_mode(profile, model);
    let transport = transport_for(api_mode);
    // F-2 — run the per-provider request-shaping hooks (previously dead code). prepare_messages runs
    // BEFORE the transport converts the transcript (per-provider message prep — e.g. Qwen cache_control).
    let prepared = profile.hooks.prepare_messages(messages.to_vec());
    let tools_opt = if tools.is_empty() { None } else { Some(tools) };
    let mut body = transport.build_kwargs(model, &prepared, tools_opt, profile, params);
    // Merge the provider's `extra_body`, then its split reasoning kwargs (an `extra_body` part + a
    // top-level part) — e.g. OpenRouter provider prefs, Kimi top-level reasoning_effort.
    let ctx = providers::hooks::ReqCtx { session_id: params.session_id.clone(), model: model.to_string() };
    deep_merge_object(&mut body, profile.hooks.build_extra_body(params.session_id.as_deref(), &ctx));
    let (extra_body, top_level) = profile.hooks.build_api_kwargs_extras(params.reasoning.as_ref(), &ctx);
    deep_merge_object(&mut body, extra_body);
    if let Some(obj) = body.as_object_mut() {
        for (k, v) in top_level {
            obj.insert(k, v);
        }
    }
    // Reasoning effort (DeepSeek/Kimi: `thinking:{disabled}` for Minimal, else `reasoning_effort`).
    // The Responses transport writes its NATIVE `reasoning:{effort}` object in build_kwargs, so skip the
    // chat-shaped `apply_reasoning_effort` hook there — /v1/responses doesn't accept the chat key (RESP-R5).
    if api_mode != ApiMode::Responses
        && let Some(effort) = params.reasoning.as_ref().and_then(|r| r.effort)
    {
        profile.hooks.apply_reasoning_effort(&mut body, effort);
    }
    // For AnthropicMessages with caching: annotate the already-built body's content blocks in place
    // (cache_control belongs on a content block, NOT the message object — see prompt_cache).
    if profile.cache_supported && api_mode == ApiMode::AnthropicMessages {
        apply_anthropic_cache_control_to_body(&mut body);
    }
    body
}

/// Recursively merge `overlay`'s object keys into `base` (objects merge key-wise; scalars/arrays
/// overwrite). Folds a provider hook's `extra_body` into the transport-built request without clobbering
/// unrelated siblings already present.
fn deep_merge_object(base: &mut serde_json::Value, overlay: serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(existing) => deep_merge_object(existing, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) => *b = o,
    }
}

#[async_trait]
impl ProviderDispatch for HttpDispatch {
    async fn complete(
        &self,
        req: &DispatchRequest,
        cancel: CancellationToken,
    ) -> Result<NormalizedResponse, ApiError> {
        // Bound total attempts across rotate/fallback/backoff so a misconfigured chain can't spin.
        let mut attempts: u32 = 0;
        let max_attempts: u32 = 16;
        let mut last_err: ApiError =
            ApiError::Transport("no provider attempted".into());

        loop {
            if cancel.is_cancelled() {
                return Err(ApiError::Transport("cancelled".into()));
            }
            if attempts >= max_attempts {
                return Err(last_err);
            }
            attempts += 1;

            // 1. current route.
            let (provider, model, chain_api_mode) = self.current_link();

            // 2. profile (fallback to a generic ChatCompletions profile when unregistered). The
            //    registered profile's api_mode wins; an unregistered provider keeps the chain's.
            let owned_fallback;
            let profile: &ProviderProfile = match self.registry.get(&provider) {
                Some(p) => p,
                None => {
                    owned_fallback = fallback_profile(&provider);
                    &owned_fallback
                }
            };
            let api_mode = if self.registry.get(&provider).is_some() {
                providers::effective_api_mode(profile, &model) // RESP-R2 — per-model wire (gpt-5.x→Responses)
            } else {
                chain_api_mode
            };

            // 3. pool + credential selection.
            let mut pool = match self.resolver.pool_for(req.tenant, &provider).await {
                Ok(p) => p,
                Err(_) => {
                    // No creds for this provider → try to fall back; else abort.
                    if self.advance_chain() {
                        continue;
                    }
                    return Err(ApiError::Transport(format!("no credentials for {provider}")));
                }
            };
            let now_ms = chrono::Utc::now().timestamp_millis();
            let (cred_id, key_hint) = match pool.select(now_ms) {
                Some(c) => (c.id, c.key_hint.clone()),
                None => {
                    if self.advance_chain() {
                        continue;
                    }
                    return Err(ApiError::Transport(format!(
                        "no eligible credential for {provider}"
                    )));
                }
            };

            // 4. decrypt → zeroizing key.
            let key = match self.resolver.decrypt(req.tenant, cred_id).await {
                Ok(k) => k,
                Err(_) => {
                    if self.advance_chain() {
                        continue;
                    }
                    return Err(ApiError::Transport(format!("decrypt failed for {provider}")));
                }
            };
            // Hold the derived plaintext in a zeroize-on-drop buffer so this lingering copy is wiped when
            // the loop iteration ends (S1-R38). The auth-header bytes still reach reqwest's HeaderValue,
            // which we don't control, so this shrinks — not eliminates — the in-memory key lifetime.
            let key_str = zeroize::Zeroizing::new(String::from_utf8_lossy(key.expose()).into_owned());

            // 5/6/7. build the outbound body (cache_control applied inside when supported).
            let body =
                build_outbound_body(profile, &model, &req.messages, &req.tools, &req.params);

            // 8. URL + headers.
            let url = http::client::versioned_base_url(&profile.base_url, endpoint_for(api_mode));
            if http::client::validate_base_url_security(&profile.base_url, self.allow_insecure)
                .is_err()
            {
                return Err(ApiError::Transport(format!(
                    "insecure base_url rejected for {provider}"
                )));
            }
            let mut request = self.client.post(&url).json(&body);
            // static default headers (e.g. anthropic-version).
            for (k, v) in &profile.default_headers {
                request = request.header(k, v);
            }
            // auth header: the env_http_headers map names WHICH header carries the key. Authorization
            // gets a `Bearer ` prefix; x-api-key (Anthropic) carries the raw key.
            for header_name in profile.env_http_headers.keys() {
                if header_name.eq_ignore_ascii_case("Authorization") {
                    request = request.header(header_name, format!("Bearer {}", key_str.as_str()));
                } else {
                    request = request.header(header_name, key_str.as_str());
                }
            }
            if profile.env_http_headers.is_empty() {
                // Default to Bearer auth on the Authorization header.
                request = request.header("Authorization", format!("Bearer {}", key_str.as_str()));
            }

            // 9. send (honor cancel via select).
            let resp = tokio::select! {
                _ = cancel.cancelled() => return Err(ApiError::Transport("cancelled".into())),
                r = request.send() => r,
            };
            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    last_err = ApiError::Transport(e.to_string());
                    // a network/transport error: treat as retryable backoff once, else fallback.
                    if self.advance_chain() {
                        continue;
                    }
                    return Err(last_err);
                }
            };

            let status = resp.status();
            // S1-R34 — capture the Retry-After header BEFORE consuming the body, so a real provider
            // reset hint (delta-seconds or an HTTP-date) drives the cooldown instead of a flat default.
            let retry_after_ms = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_retry_after_ms);
            let text = resp.text().await.unwrap_or_default();

            // 10. success → normalize.
            if status.is_success() {
                // S1-R35 — the selected credential just worked: heal it in-memory. Only persist when
                // `mark_ok` reports an ACTUAL heal (an Exhausted→Ok transition) — the common case is an
                // already-`Ok` cred, where the in-memory no-op needs no DB round-trip (begin + SET
                // tenant GUC + SELECT creds + commit). When a cooldown WAS cleared, persist so the key
                // recovers on disk instead of staying stuck until the user re-saves it in Settings. The
                // mutable `mark_ok` borrow completes before the shared `persist_transitions(&pool)` one.
                if pool.mark_ok(&key_hint) {
                    let _ = self.resolver.persist_transitions(req.tenant, &provider, &pool).await;
                }
                let json: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => return Err(ApiError::Decode(e.to_string())),
                };
                let transport = transport_for(api_mode);
                return transport
                    .normalize_response(&json)
                    .map_err(|e| ApiError::Decode(e.to_string()));
            }

            // 11. error → classify → decide_action.
            let err = ApiError::Status { status: status.as_u16(), body: text };
            last_err = err.clone();
            let ce = classify(&err, &ClassifyCtx { provider: provider.clone(), retry_after_ms });
            match decide_action(&ce) {
                Action::Rotate => {
                    let has_next = pool
                        .mark_exhausted_and_rotate(
                            status.as_u16(),
                            ce.reset_at_ms.map(|d| now_ms + d),
                            Some(&key_hint),
                            now_ms,
                        )
                        .is_some();
                    let _ = self
                        .resolver
                        .persist_transitions(req.tenant, &provider, &pool)
                        .await;
                    if has_next {
                        // another credential on the SAME provider is available → retry same link.
                        continue;
                    }
                    // pool exhausted → fall back to the next provider; else abort.
                    if self.advance_chain() {
                        continue;
                    }
                    return Err(err);
                }
                Action::Fallback | Action::Compress => {
                    // Compress is reserved (no compressor wired here yet) → treat as a fallback hop.
                    if self.advance_chain() {
                        continue;
                    }
                    return Err(err);
                }
                Action::Backoff => {
                    let delay_ms = ce.reset_at_ms.unwrap_or(500).clamp(0, 30_000) as u64;
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(ApiError::Transport("cancelled".into())),
                        _ = tokio::time::sleep(std::time::Duration::from_millis(delay_ms)) => {}
                    }
                    continue;
                }
                Action::Abort => {
                    if matches!(ce.reason, FailoverReason::AuthPermanent) {
                        pool.mark_dead(&key_hint);
                        pool.set_dead_at(&key_hint, now_ms);
                        let _ = self
                            .resolver
                            .persist_transitions(req.tenant, &provider, &pool)
                            .await;
                    }
                    return Err(err);
                }
            }
        }
    }
}

/// Parse an HTTP `Retry-After` value to ms-from-now: either delta-seconds ("120") or an HTTP-date.
fn parse_retry_after_ms(v: &str) -> Option<i64> {
    let v = v.trim();
    if let Ok(secs) = v.parse::<i64>() {
        // saturating: a hostile/garbage provider value must not overflow (panic in debug / wrap in release).
        return Some(secs.max(0).saturating_mul(1000));
    }
    let when = httpdate::parse_http_date(v).ok()?;
    let now = std::time::SystemTime::now();
    when.duration_since(now).ok().map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
