//! QCue S3 — assemble the app router from the sub-routers + global middleware + state.
//!
//! Surface: auth routes + health + global concerns + the protected capture / vault / jobs surfaces
//! (this S3 milestone). The recall/wiki/dream/sync SSE surfaces land in later milestones. Every
//! protected route goes through the dual-JWT `TenantCtx` extractor (RLS GUC bound per request tx).
use crate::account;
use crate::activity;
use crate::auth;
use crate::capture;
use crate::conversations;
use crate::dream;
use crate::health;
use crate::ingest;
use crate::jobs;
use crate::legal;
use crate::middleware as mw;
use crate::release;
use crate::settings;
use crate::state::AppState;
use crate::sync;
use crate::transcribe;
use crate::vault;
use crate::wellknown;
use crate::wikiapi;
use crate::wire;
use axum::extract::DefaultBodyLimit;
use axum::Router;

/// The protected v1 surface (every route goes through the dual-JWT `TenantCtx` extractor + RLS GUC).
/// Includes the S3-finish SSE streams (recall / wiki-query / dream / ingest) + the CRDT sync hub + the
/// Dream manual-run surface; the SSE GET routes honour the `?token=` allowlist inside the extractor.
fn protected() -> Router<AppState> {
    Router::new()
        .merge(account::routes::routes())
        .merge(capture::routes::routes())
        .merge(ingest::routes::routes())
        .merge(vault::routes::routes())
        .merge(jobs::routes::routes())
        .merge(wire::routes::routes())
        .merge(sync::routes::routes())
        .merge(dream::routes::routes())
        .merge(wikiapi::routes::routes())
        .merge(activity::routes::routes())
        .merge(conversations::routes::routes())
        .merge(settings::routes::routes())
        .merge(transcribe::routes::routes())
        .merge(release::protected_routes())
}

/// Build the full axum router from state (auth + health + protected surfaces + global middleware).
pub fn build_router(state: AppState) -> Router {
    let mut app = Router::new()
        .merge(health::routes())
        .merge(legal::routes())
        .merge(release::public_routes())
        .merge(wellknown::public_routes())
        .merge(auth::routes::routes())
        .merge(protected());

    // Security headers on every response (S3-R61).
    for layer in mw::security_headers() {
        app = app.layer(layer);
    }
    // Tag JSON responses `; charset=utf-8` so latin-1-defaulting clients decode UTF-8 (CJK) correctly.
    app = app.layer(axum::middleware::from_fn(mw::json_charset));

    app.layer(axum::middleware::from_fn_with_state(state.clone(), mw::origin_reject))
        .layer(axum::middleware::from_fn_with_state(state.clone(), mw::rate_limit))
        .layer(DefaultBodyLimit::max(mw::BODY_LIMIT_BYTES))
        .with_state(state)
}
