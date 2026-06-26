//! QCue app-server — the axum 0.8 multi-tenant backend skeleton (auth + RLS + global concerns).
//! Heavy logic (capture/recall/dream/sync/jobs) lives in the S1/S2 crates this server calls; the
//! later S3 milestones add those surfaces on top of this foundation.
pub mod account;
pub mod activity;
pub mod auth;
pub mod capture;
pub mod config;
pub mod conversations;
pub mod db;
pub mod dispatch;
pub mod dream;
pub mod error;
pub mod health;
pub mod ingest;
pub mod jobs;
pub mod legal;
pub mod middleware;
pub mod objstore;
pub mod recall;
pub mod recall_tools;
pub mod web_tool;
pub mod redact;
pub mod release;
pub mod router;
pub mod settings;
pub mod state;
pub mod sync;
pub mod tenancy;
pub mod transcribe;
pub mod vault;
pub mod wellknown;
pub mod wikiapi;
pub mod wire;
