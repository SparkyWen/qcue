//! QCue S3 — the wire surface: the backend↔client decoupling.
//!
//! `dispatch` is the per-connection JSON-RPC-lite decision layer (init gate + opt-out + `-32001`
//! backpressure); `engine` is the per-stream `Op`-in/`Event`-out actor with one serializing writer
//! (a slow client back-pressures only its own stream); `replay` is the 20-event replay ring for
//! replay-on-reconnect; `hub` is the per-stream broadcast+replay registry the SSE routes subscribe to;
//! `sse` builds the axum `Sse` response with the 15s heartbeat; `routes` mounts the SSE GET endpoints
//! (recall / wiki-query / dream / ingest / sync-pull) behind the `?token=` allowlisted `TenantCtx`;
//! `path_guard` is the tenant realpath read-isolation guard.
pub mod dispatch;
pub mod engine;
pub mod hub;
pub mod path_guard;
pub mod replay;
pub mod routes;
pub mod sse;
pub mod ws;
