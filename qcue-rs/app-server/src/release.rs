//! AU-R6/R8/R11 — app release manifest + private-repo APK proxy. The manifest is global, non-tenant,
//! secret-free metadata served unauthenticated (alongside /version/legal). The APK proxy is JWT-gated.
use crate::state::AppState;
use app_server_protocol::v1::AppReleaseManifest;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use std::sync::OnceLock;
use std::time::Duration;

/// Upstream APKs are tens of MB; reject an absurdly large body before streaming (defensive — the asset
/// URL is operator-controlled via the manifest, not user input, but a misconfig shouldn't stream unbounded).
const MAX_APK_BYTES: u64 = 300 * 1024 * 1024;

#[derive(Debug, serde::Deserialize, Default)]
struct ManifestFile {
    android: Option<PlatformEntry>,
    ios: Option<PlatformEntry>,
}

#[derive(Debug, serde::Deserialize, Clone)]
struct PlatformEntry {
    latest_build: u32,
    latest_version: String,
    min_supported_build: u32,
    changelog: String,
    /// Android only: the GitHub *asset API* URL of the APK for `latest_build` (proxied by `get_apk`).
    apk_asset_url: Option<String>,
    /// Android only: the expected SHA-256 (lowercase hex) of the APK; surfaced to the client so it can
    /// verify the sideloaded download's integrity. Absent in older manifests ⇒ None (unverified).
    apk_sha256: Option<String>,
    /// iOS only: the App Store deep link.
    app_store_url: Option<String>,
    published_at: String,
}

/// The benign "no update info" manifest — returned when the file is missing/unreadable or has no entry
/// for `platform`. latest_build:0 means "never newer than the client", so it never nudges or force-gates.
fn empty_manifest(platform: &str) -> AppReleaseManifest {
    AppReleaseManifest {
        platform: platform.to_string(),
        latest_build: 0,
        latest_version: String::new(),
        min_supported_build: 0,
        changelog: String::new(),
        android_apk_path: None,
        android_apk_sha256: None,
        ios_app_store_url: None,
        published_at: String::new(),
    }
}

/// Read the on-disk manifest and project it onto the public per-platform wire shape. File I/O only; any
/// error path returns `empty_manifest` so a missing file never blocks clients (AU-R8).
fn load_manifest(path: &str, platform: &str) -> AppReleaseManifest {
    if path.is_empty() {
        return empty_manifest(platform);
    }
    let Ok(raw) = std::fs::read_to_string(path) else { return empty_manifest(platform) };
    let Ok(file) = serde_json::from_str::<ManifestFile>(&raw) else { return empty_manifest(platform) };
    let entry = match platform {
        "ios" => file.ios,
        _ => file.android, // default/unknown ⇒ android
    };
    let Some(e) = entry else { return empty_manifest(platform) };
    AppReleaseManifest {
        platform: platform.to_string(),
        latest_build: e.latest_build,
        latest_version: e.latest_version,
        min_supported_build: e.min_supported_build,
        changelog: e.changelog,
        // Never expose the raw GitHub asset URL; the app downloads via our JWT-gated proxy.
        android_apk_path: if platform == "ios" { None } else { Some(format!("/v1/app/apk/{}", e.latest_build)) },
        // Android only: the expected APK hash for the client's integrity check (None on iOS / if unset).
        android_apk_sha256: if platform == "ios" { None } else { e.apk_sha256 },
        ios_app_store_url: if platform == "ios" { e.app_store_url } else { None },
        published_at: e.published_at,
    }
}

#[derive(serde::Deserialize)]
struct PlatformQuery {
    platform: Option<String>,
}

/// GET /v1/app/release?platform=android|ios — unauthenticated public metadata (AU-R6).
async fn get_release(State(st): State<AppState>, Query(q): Query<PlatformQuery>) -> Json<AppReleaseManifest> {
    let platform = q.platform.as_deref().unwrap_or("android");
    Json(load_manifest(&st.cfg.release_manifest_path, platform))
}

/// Mounted on the UNauthenticated surface (alongside /version, /privacy).
pub fn public_routes() -> Router<AppState> {
    Router::new().route("/v1/app/release", get(get_release))
}

/// Resolve the GitHub asset URL for `build`, but ONLY if it is the manifest's current `latest_build`
/// (we keep no archive of old APKs). Returns None ⇒ the handler answers 404.
fn resolve_apk_asset_url(path: &str, build: u32) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let file: ManifestFile = serde_json::from_str(&raw).ok()?;
    let e = file.android?;
    if e.latest_build == build { e.apk_asset_url } else { None }
}

/// GET /v1/app/apk/{build} — JWT-gated (the `_ctx: TenantCtx` extractor enforces a valid token; the
/// tenant is irrelevant here). Streams the private-repo APK by proxying the GitHub asset API with the
/// server token (AU-R11). The token is sent ONLY to api.github.com and never logged (S1-R38).
async fn get_apk(
    State(st): State<AppState>,
    _ctx: crate::tenancy::TenantCtx,
    Path(build): Path<u32>,
) -> Response {
    let Some(asset_url) = resolve_apk_asset_url(&st.cfg.release_manifest_path, build) else {
        return (StatusCode::NOT_FOUND, "unknown build").into_response();
    };
    let Some(token) = st.cfg.github_token.clone() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "apk source not configured").into_response();
    };
    let resp = apk_proxy_client()
        .get(&asset_url)
        .header(reqwest::header::AUTHORIZATION, format!("token {token}"))
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .header(reqwest::header::USER_AGENT, "qcue-app-server")
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {
            // Defensive size bound before streaming an unbounded body (the client also bounds time).
            if r.content_length().is_some_and(|len| len > MAX_APK_BYTES) {
                return (StatusCode::BAD_GATEWAY, "apk too large").into_response();
            }
            let body = axum::body::Body::from_stream(r.bytes_stream());
            (
                [(axum::http::header::CONTENT_TYPE, "application/vnd.android.package-archive")],
                body,
            )
                .into_response()
        }
        _ => (StatusCode::BAD_GATEWAY, "apk fetch failed").into_response(),
    }
}

/// Shared, connection-pooled client for the APK proxy (S1-R38 DoS bounds): a connect timeout bounds a
/// dead upstream, and a generous total timeout bounds a hung/slow stream without cutting off a legitimate
/// tens-of-MB APK download. Built once (the proxy is a low-frequency path — Shorebird covers the common case).
fn apk_proxy_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap_or_default()
    })
}

/// Mounted INSIDE the protected surface so the `TenantCtx` extractor requires a valid JWT.
pub fn protected_routes() -> Router<AppState> {
    Router::new().route("/v1/app/apk/{build}", get(get_apk))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn missing_file_degrades_to_empty_manifest() {
        let m = load_manifest("/nonexistent/qcue-release.json", "android");
        assert_eq!(m.latest_build, 0);
        assert_eq!(m.min_supported_build, 0);
        assert!(m.android_apk_path.is_none());
    }

    #[test]
    fn empty_path_degrades_to_empty_manifest() {
        let m = load_manifest("", "ios");
        assert_eq!(m.platform, "ios");
        assert_eq!(m.latest_build, 0);
    }

    #[test]
    fn android_entry_projects_apk_path_not_raw_url() {
        let dir = std::env::temp_dir();
        let p = dir.join("qcue-release-test-android.json");
        std::fs::write(&p, r#"{"android":{"latest_build":10,"latest_version":"1.0.4","min_supported_build":9,"changelog":"x","apk_asset_url":"https://api.github.com/repos/SparkyWen/qcue/releases/assets/1","published_at":"2026-06-24T00:00:00Z"}}"#).unwrap();
        let m = load_manifest(p.to_str().unwrap(), "android");
        assert_eq!(m.latest_build, 10);
        assert_eq!(m.android_apk_path.as_deref(), Some("/v1/app/apk/10"));
        assert!(m.ios_app_store_url.is_none());
        // the secret-ish GitHub URL must never reach the wire shape
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("github.com"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn android_entry_projects_apk_sha256_for_integrity_check() {
        // Mobile-audit follow-up: the client verifies the sideloaded APK against an expected SHA-256.
        // The manifest must carry `android_apk_sha256` (android only) so the client can detect a
        // tampered/MITM'd download. A manifest without the field degrades to None (back-compat).
        let dir = std::env::temp_dir();
        let p = dir.join("qcue-release-test-apksha.json");
        let sha = "a".repeat(64);
        std::fs::write(
            &p,
            format!(
                r#"{{"android":{{"latest_build":10,"latest_version":"1.0.4","min_supported_build":9,"changelog":"x","apk_asset_url":"https://api.github.com/repos/SparkyWen/qcue/releases/assets/1","apk_sha256":"{sha}","published_at":"t"}}}}"#
            ),
        )
        .unwrap();
        let m = load_manifest(p.to_str().unwrap(), "android");
        assert_eq!(m.android_apk_sha256.as_deref(), Some(sha.as_str()));
        // iOS never carries an APK hash.
        let mi = load_manifest(p.to_str().unwrap(), "ios");
        assert!(mi.android_apk_sha256.is_none());
        // A missing field degrades to None, never an error.
        let m2 = load_manifest("/nonexistent/qcue-release.json", "android");
        assert!(m2.android_apk_sha256.is_none());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn apk_build_must_match_latest_or_404() {
        // The proxy only serves the manifest's `latest_build`; any other build is 404 (no old-APK store).
        let dir = std::env::temp_dir();
        let p = dir.join("qcue-release-test-apkmatch.json");
        std::fs::write(&p, r#"{"android":{"latest_build":10,"latest_version":"1.0.4","min_supported_build":9,"changelog":"x","apk_asset_url":"https://api.github.com/repos/SparkyWen/qcue/releases/assets/1","published_at":"t"}}"#).unwrap();
        assert_eq!(
            resolve_apk_asset_url(p.to_str().unwrap(), 10).as_deref(),
            Some("https://api.github.com/repos/SparkyWen/qcue/releases/assets/1")
        );
        assert!(resolve_apk_asset_url(p.to_str().unwrap(), 9).is_none()); // not the latest ⇒ None ⇒ 404
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn ios_entry_projects_store_url() {
        let dir = std::env::temp_dir();
        let p = dir.join("qcue-release-test-ios.json");
        std::fs::write(&p, r#"{"ios":{"latest_build":12,"latest_version":"1.0.5","min_supported_build":11,"changelog":"y","app_store_url":"itms-apps://apple.com/app/id6783192160","published_at":"2026-06-24T00:00:00Z"}}"#).unwrap();
        let m = load_manifest(p.to_str().unwrap(), "ios");
        assert_eq!(m.latest_build, 12);
        assert_eq!(m.ios_app_store_url.as_deref(), Some("itms-apps://apple.com/app/id6783192160"));
        assert!(m.android_apk_path.is_none());
        std::fs::remove_file(&p).ok();
    }
}
