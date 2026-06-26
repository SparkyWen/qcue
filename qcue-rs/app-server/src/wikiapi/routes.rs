//! QCue S3 — the wiki READ surface the Flutter client consumes (the contract in
//! `qcue_app/lib/core/net/http_api_client.dart`):
//!   - `GET /v1/wiki/pages`        → `{pages:[WikiPage]}` index (title+summary, no body)
//!   - `GET /v1/wiki/pages/{slug}` → a single `WikiPage` (body_markdown + backlinks); 404 ⇒ null.
//!
//! The mirror rows live in `wiki_pages` (slug/title/summary/type/aliases/tags/updated, all tenant-scoped
//! under FORCE RLS via the per-request GUC); the markdown body is the source-of-truth in the vault at
//! `body_ref` and is read root-confined (the stored absolute path must resolve UNDER this tenant's vault
//! root — never trust a path into the filesystem blindly, defense-in-depth like the recall query engine).
//! Backlinks are the INCOMING edges from `wiki_links` (rows whose `target_page_id` = this page).
use crate::error::ApiError;
use crate::redact::redact_json;
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use sqlx::Row;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/wiki/pages", get(index))
        .route("/v1/wiki/pages/{slug}", get(page))
}

/// Pass the outbound body through the central redactor before it leaves the server (defense in depth).
fn redacted(mut v: serde_json::Value) -> Json<serde_json::Value> {
    redact_json(&mut v);
    Json(v)
}

/// `GET /v1/wiki/pages` — the index list: every non-deleted page with its metadata (no body). Maps to
/// the Dart `WikiPage` shape (`body_markdown` empty / `backlinks` empty for the list projection).
async fn index(mut ctx: TenantCtx) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, type::text AS type, slug, title, summary, aliases, tags, updated \
         FROM wiki_pages WHERE deleted_at IS NULL ORDER BY updated DESC LIMIT 2000",
    )
    .fetch_all(&mut *ctx.tx)
    .await?;
    let pages: Vec<_> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "type": r.get::<String, _>("type"),
                "slug": r.get::<String, _>("slug"),
                "title": r.get::<String, _>("title"),
                "summary": r.get::<String, _>("summary"),
                "body_markdown": "",
                "updated": r.get::<chrono::DateTime<chrono::Utc>, _>("updated"),
                "aliases": r.get::<Vec<String>, _>("aliases"),
                "tags": r.get::<Vec<String>, _>("tags"),
                "backlinks": [],
            })
        })
        .collect();
    ctx.tx.commit().await?;
    Ok(redacted(serde_json::json!({ "pages": pages })))
}

/// `GET /v1/wiki/pages/{slug}` — a single page resolved by slug OR alias, with its markdown body
/// (root-confined read) + incoming backlinks. 404 ⇒ the Dart client maps it to `null`.
async fn page(
    State(st): State<AppState>,
    mut ctx: TenantCtx,
    Path(slug): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row = sqlx::query(
        "SELECT id, type::text AS type, slug, title, summary, aliases, tags, updated, body_ref \
         FROM wiki_pages WHERE deleted_at IS NULL AND (slug=$1 OR $1 = ANY(aliases)) LIMIT 1",
    )
    .bind(&slug)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    let Some(r) = row else {
        ctx.tx.commit().await?;
        return Err(ApiError::NotFound);
    };
    let id: Uuid = r.get("id");
    // backlinks = INCOMING edges: other pages whose [[wikilink]] resolved to this page id. Each carries
    // the source page's slug as the link target_slug (so the app can navigate back). target_page_id is
    // always this page (a live link); display is the source page's title for a readable label.
    let blinks = sqlx::query(
        "SELECT p.slug AS src_slug, p.id AS src_id, p.title AS src_title \
         FROM wiki_links l JOIN wiki_pages p ON p.id = l.src_page_id AND p.deleted_at IS NULL \
         WHERE l.target_page_id = $1 ORDER BY p.title",
    )
    .bind(id)
    .fetch_all(&mut *ctx.tx)
    .await?;
    let backlinks: Vec<_> = blinks
        .iter()
        .map(|b| {
            serde_json::json!({
                "target_slug": b.get::<String, _>("src_slug"),
                "target_page_id": b.get::<Uuid, _>("src_id"),
                "display": b.get::<String, _>("src_title"),
            })
        })
        .collect();
    let body_ref: String = r.get("body_ref");
    // Read the markdown body root-confined: the stored absolute path must resolve UNDER this tenant's
    // vault root (the mirror is system-set, but never blindly trust a path into the filesystem). A
    // missing/out-of-root body yields an empty body_markdown rather than an error (the page still lists).
    let body_markdown = read_body_confined(&st, ctx.tenant_id, &body_ref).await;
    let out = serde_json::json!({
        "id": id,
        "type": r.get::<String, _>("type"),
        "slug": r.get::<String, _>("slug"),
        "title": r.get::<String, _>("title"),
        "summary": r.get::<String, _>("summary"),
        "body_markdown": body_markdown,
        "updated": r.get::<chrono::DateTime<chrono::Utc>, _>("updated"),
        "aliases": r.get::<Vec<String>, _>("aliases"),
        "tags": r.get::<Vec<String>, _>("tags"),
        "backlinks": backlinks,
    });
    ctx.tx.commit().await?;
    Ok(redacted(out))
}

/// Read `body_ref` (the stored absolute vault path) iff it resolves under this tenant's vault root.
/// Out-of-root / missing → `""` (skip the load; never escape the root, never error the whole page).
async fn read_body_confined(st: &AppState, tenant: Uuid, body_ref: &str) -> String {
    let root = st.vault_root(tenant);
    match (std::fs::canonicalize(&root), std::fs::canonicalize(body_ref)) {
        (Ok(root), Ok(path)) if path.starts_with(&root) => {
            tokio::fs::read_to_string(&path).await.unwrap_or_default()
        }
        _ => String::new(),
    }
}
