//! QCue S3-R66 — health probes. /healthz = liveness (always 200); /readyz = DB+migrations (503 until up).
//! /version = build provenance (git SHA + build time), so "is prod running the merged code?" is a one-line
//! `curl`, not an SSH+grep investigation. See docs/postmortems/2026-06-17-stale-binary-incident.md.
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        .route("/readyz", get(readyz))
        .route("/version", get(version))
}

/// Build provenance baked in at compile time by `build.rs`. Unauthenticated by design — it exposes only
/// the git SHA / build time of the running binary (no secrets), which is exactly what a deploy check and a
/// drift monitor need. The `deploy-prod.sh` SHA assertion reads `.sha` from this.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VersionInfo {
    /// Full git commit SHA the binary was built from (`"unknown"` only if built outside a git tree).
    pub sha: &'static str,
    /// First 12 chars of `sha`, for human-friendly comparison.
    pub short_sha: &'static str,
    /// Were there uncommitted changes in the work tree at build time? (`"true"`/`"false"`/`"unknown"`.)
    pub dirty: &'static str,
    /// RFC-3339 UTC build time (or the source commit time as a reproducible fallback).
    pub built_at: &'static str,
    /// The `app-server` crate version (`CARGO_PKG_VERSION`).
    pub pkg_version: &'static str,
}

/// The compiled-in build provenance. PURE — all fields are `env!`-resolved constants from `build.rs`.
pub fn build_info() -> VersionInfo {
    let sha = env!("QCUE_GIT_SHA");
    VersionInfo {
        sha,
        short_sha: if sha.len() >= 12 { &sha[..12] } else { sha },
        dirty: env!("QCUE_GIT_DIRTY"),
        built_at: env!("QCUE_BUILD_TIME"),
        pkg_version: env!("CARGO_PKG_VERSION"),
    }
}

async fn version() -> Json<VersionInfo> {
    Json(build_info())
}

async fn readyz(State(st): State<AppState>) -> StatusCode {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&st.pool).await.is_ok();
    // The latest in-scope migration owning the auth schema is `M0_0002_users`; readiness requires it.
    let mig_ok = crate::db::migrations_applied(&st.pool, "users").await;
    if db_ok && mig_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    // The whole point of /version is to make "is prod running the code I merged?" answerable without
    // SSH+grep. That only works if the git SHA is genuinely embedded at build time (not "unknown").
    #[test]
    fn build_info_embeds_a_real_git_sha_at_build_time() {
        let v = build_info();
        assert!(
            !v.sha.is_empty() && v.sha != "unknown",
            "git SHA must be embedded at build time (build.rs), got {:?} — \
             a stale/missing SHA defeats the whole stale-binary guard",
            v.sha
        );
        assert_eq!(v.pkg_version, env!("CARGO_PKG_VERSION"));
        assert!(v.short_sha.len() <= v.sha.len() && v.sha.starts_with(v.short_sha));
    }

    #[test]
    fn version_json_exposes_the_documented_keys() {
        let j = serde_json::to_value(build_info()).unwrap();
        for k in ["sha", "short_sha", "dirty", "built_at", "pkg_version"] {
            assert!(j.get(k).is_some(), "/version JSON must expose `{k}` (deploy script asserts on `sha`)");
        }
    }
}
