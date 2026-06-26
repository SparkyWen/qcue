//! Mobile-audit follow-up — serve `/.well-known/assetlinks.json` (Android App Links / Digital Asset
//! Links). App Links bind verified `https://` deep links exclusively to the signed app, closing the
//! `qcue://` OAuth-redirect hijack window (see `qcue_app/android/APP_LINKS.md`). The file content is the
//! operator's signing-cert fingerprints — secret-free, unauthenticated, served verbatim from a configured
//! path (`QCUE_ASSETLINKS_PATH`). Unset/missing ⇒ 404 (never a 5xx; the App Links flow simply isn't live
//! until the operator drops the file in place, per the coordinated runbook).
use crate::state::AppState;
use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

/// Read the on-disk assetlinks JSON. File I/O only; an empty path or any read error ⇒ None (the route
/// then answers 404). The bytes are served verbatim — this server never synthesizes fingerprints.
fn load_assetlinks(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// GET /.well-known/assetlinks.json — unauthenticated public metadata. 200 `application/json` when the
/// operator has configured the file, else 404. Served with NO redirect (App Links verification requires a
/// direct 200 — `APP_LINKS.md` §2).
async fn get_assetlinks(State(st): State<AppState>) -> Response {
    match load_assetlinks(&st.cfg.assetlinks_path) {
        Some(body) => (
            [(header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "assetlinks not configured").into_response(),
    }
}

/// Mounted on the UNauthenticated surface (alongside /version, /privacy, /v1/app/release).
pub fn public_routes() -> Router<AppState> {
    Router::new().route("/.well-known/assetlinks.json", get(get_assetlinks))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn unset_or_missing_path_yields_none() {
        assert!(load_assetlinks("").is_none(), "empty path ⇒ None (route 404s)");
        assert!(load_assetlinks("/nonexistent/assetlinks.json").is_none(), "missing file ⇒ None");
    }

    #[test]
    fn configured_file_is_served_verbatim() {
        let dir = std::env::temp_dir();
        let p = dir.join("qcue-test-assetlinks.json");
        let body = r#"[{"relation":["delegate_permission/common.handle_all_urls"],"target":{"namespace":"android_app","package_name":"cn.qcue.app","sha256_cert_fingerprints":["AA:BB"]}}]"#;
        std::fs::write(&p, body).unwrap();
        assert_eq!(load_assetlinks(p.to_str().unwrap()).as_deref(), Some(body));
        std::fs::remove_file(&p).ok();
    }
}
