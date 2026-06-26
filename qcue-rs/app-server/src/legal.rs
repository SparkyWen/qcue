//! QCue — public legal pages. Unauthenticated, served so the App Store privacy URL
//! (`https://app.qcue.cn/privacy`) resolves. The HTML is the committed source of truth under
//! `docs/legal/`, baked into the binary via `include_str!` (single-source, no runtime file I/O).
//! These routes are merged onto the UNauthenticated surface (alongside `/healthz`), never behind
//! the `TenantCtx` extractor — a privacy policy must be readable without an account.
use crate::state::AppState;
use axum::response::Html;
use axum::routing::get;
use axum::Router;

// include_str! is resolved relative to THIS source file (qcue-rs/app-server/src/legal.rs); the repo
// root is three hops up (src → app-server → qcue-rs → repo root), then docs/legal/.
const PRIVACY_EN: &str = include_str!("../../../docs/legal/privacy-policy.en.html");
const PRIVACY_ZH: &str = include_str!("../../../docs/legal/privacy-policy.zh.html");
const SUPPORT: &str = include_str!("../../../docs/legal/support.html");
const DELETE_ACCOUNT: &str = include_str!("../../../docs/legal/delete-account.html");

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/privacy", get(privacy_en))
        .route("/privacy/zh", get(privacy_zh))
        .route("/support", get(support))
        .route("/delete-account", get(delete_account))
}

/// GET /privacy — the English privacy policy (App Store privacy URL target).
async fn privacy_en() -> Html<&'static str> {
    Html(PRIVACY_EN)
}

/// GET /privacy/zh — the Chinese privacy policy.
async fn privacy_zh() -> Html<&'static str> {
    Html(PRIVACY_ZH)
}

/// GET /support — public support/contact page (App Store Support URL target).
async fn support() -> Html<&'static str> {
    Html(SUPPORT)
}

/// GET /delete-account — public account/data deletion instructions (Google Play Data-deletion URL).
async fn delete_account() -> Html<&'static str> {
    Html(DELETE_ACCOUNT)
}
