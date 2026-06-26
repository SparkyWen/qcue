//! QCue S3-R36 / §7.5 — the SSE GET endpoints + the recall/wiki-query drivers.
//!
//! These mount the streaming surfaces the Flutter app subscribes to via `EventSource`:
//!   - `GET /v1/recall/{thread}/stream`     — recall chat (the Appendix A §3.4 taxonomy)
//!   - `GET /v1/wiki/query/{thread}/stream`  — index-first synthesis (same engine + taxonomy)
//!   - `GET /v1/dream/{job}/stream`          — Dream-detail (`dream_started/progress/completed/failed`)
//!   - `GET /v1/ingest/{job}/stream`         — ingest progress
//!   - `GET /v1/sync/pull`                   — pull-since (handled in `sync::routes`; allowlisted here)
//!
//! Every stream authenticates through the dual-JWT `TenantCtx` extractor, which honours the `?token=`
//! SSE allowlist (`EventSource` can't send an `Authorization` header — pitfall #15). The recall driver
//! runs `wiki::recall_query` through a `RecallSink` adapter and emits the §3.4 taxonomy as
//! `RuntimeEventEnvelope`s; reasoning is streamed but flagged collapsed-by-default (D18).
use crate::error::ApiError;
use crate::recall::{run_recall_stream, RecallMode};
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use crate::wire::hub::StreamHub;
use crate::wire::sse::sse_with_backfill;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use std::convert::Infallible;
use uuid::Uuid;
use wiki::llm::RecallOverride;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/recall/{thread}/stream", get(recall_stream))
        .route("/v1/wiki/query/{thread}/stream", get(wiki_query_stream))
        .route("/v1/dream/{job}/stream", get(dream_stream))
        .route("/v1/ingest/{job}/stream", get(ingest_stream))
        // the WSS JSON-RPC-lite turn channel (upgrade GET; ?token= allowlisted for browser sockets).
        .route("/v1/thread/{thread}/ws", get(crate::wire::ws::thread_ws))
}

/// `GET /v1/recall/{thread}/stream` — drive a recall turn and stream the §3.4 taxonomy over SSE. A
/// reconnect (carrying `?since_seq=` / `Last-Event-ID`) REPLAYS the missed tail from the ring instead of
/// re-running the turn; a fresh connect spawns the producer.
async fn recall_stream(
    State(st): State<AppState>,
    Path(thread): Path<Uuid>,
    ctx: TenantCtx,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    Ok(stream_or_resume(st, thread, ctx, headers, Some(RecallMode::Recall)))
}

/// `GET /v1/wiki/query/{thread}/stream` — the same engine, the index-first synthesis stream.
async fn wiki_query_stream(
    State(st): State<AppState>,
    Path(thread): Path<Uuid>,
    ctx: TenantCtx,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    Ok(stream_or_resume(st, thread, ctx, headers, Some(RecallMode::WikiQuery)))
}

/// `GET /v1/dream/{job}/stream` — subscribe the Dream-detail screen to the job's progress channel
/// (`dream_started/progress/completed/failed`). The dream JobHandler publishes to this channel; a
/// reconnect replays the missed tail (`?since_seq=` / `Last-Event-ID`).
async fn dream_stream(
    State(st): State<AppState>,
    Path(job): Path<Uuid>,
    ctx: TenantCtx,
    headers: HeaderMap,
) -> impl IntoResponse {
    subscribe_with_resume(st.dream_streams, job, ctx, headers)
}

/// `GET /v1/ingest/{job}/stream` — subscribe to the ingest job's progress channel (reconnect replays).
async fn ingest_stream(
    State(st): State<AppState>,
    Path(job): Path<Uuid>,
    ctx: TenantCtx,
    headers: HeaderMap,
) -> impl IntoResponse {
    subscribe_with_resume(st.threads, job, ctx, headers)
}

/// Fresh connect → subscribe + spawn the recall/wiki producer; reconnect (`since`) → subscribe + replay
/// the missed tail WITHOUT re-running the turn (the turn already produced those events). Owned args so
/// the returned `Sse` stream captures no borrow.
fn stream_or_resume(
    st: AppState,
    thread: Uuid,
    ctx: TenantCtx,
    headers: HeaderMap,
    recall_mode: Option<RecallMode>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    // subscribe BEFORE spawning the producer so no early frame is missed. Scoped to ctx.tenant_id so a
    // foreign thread id yields an empty stream (no cross-tenant leak via the in-process hub).
    let rx = st.threads.subscribe(ctx.tenant_id, thread);
    match resume_seq(&ctx, &headers) {
        Some(since) => {
            let backfill = st.threads.replay_since(ctx.tenant_id, thread, since).unwrap_or_default();
            sse_with_backfill(backfill, rx)
        }
        None => {
            if let Some(mode) = recall_mode {
                let question = ctx_question(&ctx);
                let over = recall_override(&ctx);
                spawn_recall(st, ctx.tenant_id, ctx.user_id, thread, question, mode, over);
            }
            sse_with_backfill(Vec::new(), rx)
        }
    }
}

/// Subscribe to a hub stream, replaying the missed tail when the client reconnects with a resume seq.
fn subscribe_with_resume(
    hub: StreamHub,
    stream: Uuid,
    ctx: TenantCtx,
    headers: HeaderMap,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let rx = hub.subscribe(ctx.tenant_id, stream);
    let backfill = match resume_seq(&ctx, &headers) {
        Some(since) => hub.replay_since(ctx.tenant_id, stream, since).unwrap_or_default(),
        None => Vec::new(),
    };
    sse_with_backfill(backfill, rx)
}

/// The resume point for a reconnect: `?since_seq=N` is the FIRST seq the client still wants (replay
/// `seq >= N`); a `Last-Event-ID: N` header is the LAST seq it received, so it wants `seq >= N+1`.
fn resume_seq(ctx: &TenantCtx, headers: &HeaderMap) -> Option<u64> {
    if let Some(n) = ctx.query_param("since_seq").and_then(|v| v.parse::<u64>().ok()) {
        return Some(n);
    }
    headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(|last| last.saturating_add(1))
}

/// The recall question — read from the `?q=` query param (the `EventSource` URL carries it). Empty is
/// allowed: the synthesis prompt branches on an empty wiki / empty query.
fn ctx_question(ctx: &TenantCtx) -> String {
    ctx.query_param("q").unwrap_or_default()
}

/// v0.2.2 — the per-recall model/effort override from `?provider=&model=&effort=` (the composer picker).
/// Absent/blank params → an empty override (the tenant's default route + effort; identical to before).
fn recall_override(ctx: &TenantCtx) -> RecallOverride {
    recall_override_from(
        ctx.query_param("provider"),
        ctx.query_param("model"),
        ctx.query_param("effort"),
    )
}

/// Pure builder (testable without a `TenantCtx`): blank strings collapse to `None`.
fn recall_override_from(
    provider: Option<String>,
    model: Option<String>,
    effort: Option<String>,
) -> RecallOverride {
    let clean = |o: Option<String>| o.filter(|s| !s.trim().is_empty());
    RecallOverride { provider: clean(provider), model: clean(model), effort: clean(effort) }
}

/// Spawn the recall producer on its own task so the SSE response returns immediately and a slow client
/// only back-pressures its own broadcast buffer (S3-R40).
fn spawn_recall(
    st: AppState,
    tenant: Uuid,
    user: Uuid,
    thread: Uuid,
    question: String,
    mode: RecallMode,
    over: RecallOverride,
) {
    tokio::spawn(async move {
        run_recall_stream(&st, tenant, user, thread, &question, mode, over).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_override_collapses_blanks_and_keeps_values() {
        // present, non-blank → carried through.
        let o = recall_override_from(
            Some("openai".into()),
            Some("gpt-5.5".into()),
            Some("high".into()),
        );
        assert_eq!(o.provider.as_deref(), Some("openai"));
        assert_eq!(o.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(o.effort.as_deref(), Some("high"));
        assert_eq!(o.route(), Some(("openai".to_string(), "gpt-5.5".to_string())));
        assert!(!o.is_empty());

        // blank / whitespace / absent → an empty override (tenant default).
        let empty = recall_override_from(Some("".into()), Some("   ".into()), None);
        assert!(empty.is_empty());
        assert_eq!(empty.route(), None);

        // model alone (no provider) is NOT a usable route.
        let half = recall_override_from(None, Some("gpt-5.5".into()), None);
        assert_eq!(half.route(), None);
        assert!(!half.is_empty()); // still carries the model token
    }
}
