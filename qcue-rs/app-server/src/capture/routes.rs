//! QCue S3-R44/R45/R46 — the capture API. The idea row + canonical JSONL line are persisted BEFORE
//! any ingest job is enqueued (Master §3 persistence law / pitfall #19 spirit): a crash after the
//! row lands still records intent (`ingest_state='pending'` → a reaper re-enqueues). Untrusted body
//! is escaped (reserved tags) + fenced (`<untrusted_source>`) on the way in (RKM §7). The actual
//! ingest LOGIC is S2 — here we only enqueue a `jobs` row of `kind='ingest'` carrying the idea id.
use crate::error::ApiError;
use crate::jobs::queue::{enqueue, JobKind};
use crate::state::AppState;
use crate::tenancy::TenantCtx;
use app_server_protocol::v1::{CaptureDetail, CaptureParams, CapturePatch, CaptureResult};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use sqlx::Row;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/capture", post(capture))
        .route("/v1/captures", get(feed))
        .route("/v1/captures/{id}", get(detail).patch(edit).delete(delete_capture))
}

/// Escape reserved system tags so ingested content can't inject instructions (RKM §7 #2).
pub fn escape_reserved_tags(s: &str) -> String {
    s.replace("<system-reminder>", "&lt;system-reminder&gt;")
        .replace("</system-reminder>", "&lt;/system-reminder&gt;")
        .replace("<untrusted_source", "&lt;untrusted_source")
        .replace("</untrusted_source>", "&lt;/untrusted_source&gt;")
}

/// Wrap an untrusted blob for the message TAIL only (RKM §7 #1/#3).
pub fn fence_untrusted(origin: &str, body: &str) -> String {
    format!("<untrusted_source origin=\"{origin}\">{body}</untrusted_source>")
}

async fn capture(
    State(st): State<AppState>,
    mut ctx: TenantCtx,
    headers: HeaderMap,
    Json(req): Json<CaptureParams>,
) -> Result<Json<CaptureResult>, ApiError> {
    // Idempotency-Key (S5 cloud-sync): a retried capture carrying the same key dedups to the existing
    // idea (no duplicate insert/enqueue) so a flaky upload that retries lands exactly one row.
    let idem_key = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .filter(|s| !s.is_empty());

    // Fast path: if a row already exists for (tenant, key), return its ids without re-inserting. The
    // RLS GUC is already bound on `ctx.tx` (this tx is tenant-scoped), so the WHERE is tenant-safe.
    if let Some(key) = &idem_key
        && let Some(row) = sqlx::query(
            "SELECT id, ingest_job_id FROM ideas WHERE tenant_id = $1 AND idempotency_key = $2",
        )
        .bind(ctx.tenant_id)
        .bind(key)
        .fetch_optional(&mut *ctx.tx)
        .await?
    {
        let existing_id: Uuid = row.get("id");
        let existing_job: Option<Uuid> = row.try_get("ingest_job_id").ok().flatten();
        ctx.tx.commit().await?;
        return Ok(Json(CaptureResult {
            idea_id: existing_id,
            ingest_job_id: existing_job.unwrap_or(existing_id),
        }));
    }

    let idea_id = Uuid::now_v7();
    // Untrusted input: escape reserved tags before it is persisted/indexed (RKM §7 #2).
    let body = escape_reserved_tags(req.body.as_deref().unwrap_or(""));
    // LOC-R3/Part F — honor the client's ACTION-TIME instant: a capture made offline at 08:30 and
    // flushed hours later must land on the day it happened, not on `now()`. An absent/invalid value
    // falls back to server `now()` via `COALESCE($10, now())` below.
    let captured_at = req
        .captured_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&chrono::Utc)));
    // 1) persist the canonical JSONL FIRST (Master §3 persistence law) — a crash after this still
    //    records intent. The path guard + redaction live in the object store.
    let log_ref = st
        .objstore
        .append_capture(
            ctx.tenant_id,
            ctx.user_id,
            idea_id,
            &serde_json::json!({"kind": req.kind, "body": body, "origin": req.origin}),
        )
        .map_err(|e| ApiError::Other(e.into()))?;
    // 2) the ideas row (ingest_state defaults to 'pending' so a reaper re-enqueues if we crash before
    //    the enqueue commits). This is the persist-before-enqueue invariant (S3-R44). ON CONFLICT DO
    //    NOTHING + re-select makes the (tenant, idempotency_key) insert race-safe under concurrent
    //    retries: a loser re-reads the winner's row and returns its ids (no duplicate enqueue).
    let inserted = sqlx::query(
        // The arbiter is the PARTIAL unique index `ideas_tenant_idem_uidx (tenant_id, idempotency_key)
        // WHERE idempotency_key IS NOT NULL` (migration 50002). Postgres can only infer a partial index
        // as the ON CONFLICT arbiter when the SAME predicate is repeated here — omitting it raises
        // "no unique or exclusion constraint matching the ON CONFLICT specification" → a 500 on EVERY
        // capture (keyless ones included). With the predicate, a NULL-key row never conflicts (excluded
        // from the index) and a duplicate non-NULL key DO-NOTHINGs into the re-select path below.
        // captured_at = COALESCE(client action-time, now()); the three nullable location columns
        // (LOC-R1) persist the opt-in precise fix (NULL when the toggle is off / no fix).
        "INSERT INTO ideas(id,tenant_id,user_id,kind,body,source_url,log_ref,origin,idempotency_key,captured_at,lat,lng,loc_accuracy_m) \
         VALUES ($1,$2,$3,$4::idea_kind,$5,$6,$7,$8,$9,COALESCE($10, now()),$11,$12,$13) \
         ON CONFLICT (tenant_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING RETURNING id, captured_at",
    )
    .bind(idea_id)
    .bind(ctx.tenant_id)
    .bind(ctx.user_id)
    .bind(&req.kind)
    .bind(&body)
    .bind(&req.source_url)
    .bind(&log_ref)
    .bind(&req.origin)
    .bind(idem_key.as_deref())
    .bind(captured_at)
    .bind(req.lat)
    .bind(req.lng)
    .bind(req.loc_accuracy_m)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    // A race lost the insert (another concurrent retry inserted first) → re-select the winner and
    // return its ids without enqueuing a second ingest job.
    if inserted.is_none()
        && let Some(key) = &idem_key
        && let Some(row) = sqlx::query(
            "SELECT id, ingest_job_id FROM ideas WHERE tenant_id = $1 AND idempotency_key = $2",
        )
        .bind(ctx.tenant_id)
        .bind(key)
        .fetch_optional(&mut *ctx.tx)
        .await?
    {
        let existing_id: Uuid = row.get("id");
        let existing_job: Option<Uuid> = row.try_get("ingest_job_id").ok().flatten();
        ctx.tx.commit().await?;
        return Ok(Json(CaptureResult {
            idea_id: existing_id,
            ingest_job_id: existing_job.unwrap_or(existing_id),
        }));
    }
    // 3) THEN enqueue the ingest job (debounced per-idea). The ingest LOGIC is S2; we only queue it.
    let job_id = enqueue(
        &mut ctx.tx,
        ctx.tenant_id,
        Some(ctx.user_id),
        JobKind::Ingest,
        serde_json::json!({ "idea_id": idea_id }),
        Some(&format!("ingest:{idea_id}")),
    )
    .await
    .map_err(|_| ApiError::Overloaded)?;
    sqlx::query("UPDATE ideas SET ingest_job_id=$1 WHERE id=$2")
        .bind(job_id)
        .bind(idea_id)
        .execute(&mut *ctx.tx)
        .await?;
    // 4) emit a server-origin idea.create op (Task 6) so other devices surface this capture on their
    //    next INCREMENTAL pull — the app re-snapshots only on a cold pull, so only emitted ops
    //    propagate after that. site_id 0 / applied=true (the canonical row above already exists).
    if let Some(row) = &inserted {
        let captured_at: chrono::DateTime<chrono::Utc> = row.get("captured_at");
        crate::sync::emit::emit_op(
            &mut ctx.tx,
            ctx.tenant_id,
            ctx.user_id,
            "idea",
            &idea_id.to_string(),
            serde_json::json!({ "create": {
                "body": body,
                "origin": req.origin,
                "captured_at": captured_at.to_rfc3339(),
                "lat": req.lat,
                "lng": req.lng,
                "loc_accuracy_m": req.loc_accuracy_m,
            } }),
        )
        .await?;
    }
    ctx.tx.commit().await?;
    Ok(Json(CaptureResult { idea_id, ingest_job_id: job_id }))
}

/// Optional [start,end) date scope for the feed. Both RFC3339 UTC instants must be present together to
/// scope the feed to a chosen day/range (the app computes the UTC window for the user's LOCAL day so the
/// server never assumes a timezone). Absent → the default newest-N feed.
#[derive(serde::Deserialize, Default)]
pub struct FeedQuery {
    pub start: Option<String>,
    pub end: Option<String>,
}

/// A half-open `[start, end)` UTC window for scoping the feed to a day/range.
pub type UtcRange = (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>);

/// GET /captures — the reverse-chron capture feed for the caller's tenant (RLS-scoped). With
/// `?start=<rfc3339>&end=<rfc3339>` it returns ALL captures in that UTC window (the calendar/date
/// picker's "everything I captured on day X"); without it, the default newest-50.
async fn feed(
    mut ctx: TenantCtx,
    Query(q): Query<FeedQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let range = parse_range(&q)?;
    let items = query_feed(&mut ctx.tx, range).await?;
    ctx.tx.commit().await?;
    Ok(Json(serde_json::json!({ "captures": items })))
}

/// GET /v1/captures/{id} — full detail of one capture (CAP-R1). RLS-scoped via the bound GUC: the row
/// only resolves when it belongs to the caller's tenant, so an absent OR foreign-tenant id is 404.
async fn detail(mut ctx: TenantCtx, Path(id): Path<Uuid>) -> Result<Json<CaptureDetail>, ApiError> {
    let row = sqlx::query(
        "SELECT id, kind::text AS kind, body, captured_at, lat, lng, loc_accuracy_m, \
                ingest_state::text AS st, source_url, origin \
         FROM ideas WHERE id=$1 AND active",
    )
    .bind(id)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    let Some(r) = row else {
        ctx.tx.commit().await?;
        return Err(ApiError::NotFound);
    };
    // DIG-R4 — the distilled source page slug (if any). None ⇒ not yet ingested.
    let slug: Option<String> = sqlx::query_scalar(
        "SELECT slug FROM wiki_pages WHERE type='source' AND deleted_at IS NULL \
         AND source_ids @> ARRAY[$1]::uuid[] ORDER BY updated DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    ctx.tx.commit().await?;
    let detail = CaptureDetail {
        id: r.get("id"),
        kind: r.get("kind"),
        body: r.get("body"),
        captured_at: r.get::<chrono::DateTime<chrono::Utc>, _>("captured_at").to_rfc3339(),
        lat: r.try_get("lat").ok().flatten(),
        lng: r.try_get("lng").ok().flatten(),
        loc_accuracy_m: r.try_get("loc_accuracy_m").ok().flatten(),
        ingest_state: r.get("st"),
        source_url: r.try_get("source_url").ok().flatten(),
        origin: r.get("origin"),
        source_page_slug: slug,
    };
    Ok(Json(detail))
}

/// PATCH /v1/captures/{id} — edit a capture (CAP-R2). Re-ingest is driven by an ACTUAL body change,
/// not the act of editing: a changed body bumps updated_at (dirty-scan re-ingests via DIG-R4 slug
/// reuse, updating the linked page in place); an unchanged body updates only location WITHOUT
/// advancing updated_at past last_ingested_at, so no re-ingest and no token spend.
async fn edit(
    mut ctx: TenantCtx,
    Path(id): Path<Uuid>,
    Json(req): Json<CapturePatch>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(row) = sqlx::query("SELECT body FROM ideas WHERE id=$1 AND active")
        .bind(id)
        .fetch_optional(&mut *ctx.tx)
        .await?
    else {
        ctx.tx.commit().await?;
        return Err(ApiError::NotFound);
    };
    let current: String = row.get("body");
    let new_body = req.body.as_ref().map(|b| escape_reserved_tags(b));
    // A re-ingest is driven by an ACTUAL content change: Some(new) that differs from the stored body.
    let body_changed = matches!(&new_body, Some(b) if *b != current);

    if let Some(changed_body) = new_body.as_deref().filter(|_| body_changed) {
        // Body change: bump updated_at (the ideas_touch trigger does this on any UPDATE), so the
        // dirty-scan (updated_at > last_ingested_at) re-ingests and updates the linked page in place.
        sqlx::query(
            "UPDATE ideas SET body=$2, lat=COALESCE($3,lat), lng=COALESCE($4,lng), \
                    loc_accuracy_m=COALESCE($5,loc_accuracy_m) WHERE id=$1",
        )
        .bind(id)
        .bind(changed_body)
        .bind(req.lat)
        .bind(req.lng)
        .bind(req.loc_accuracy_m)
        .execute(&mut *ctx.tx)
        .await?;
    } else {
        // No body change: update location only, and DO NOT let updated_at outrun last_ingested_at
        // (keep last_ingested_at >= updated_at) so the dirty-scan stays false — true no-op for the wiki.
        sqlx::query(
            "UPDATE ideas SET lat=COALESCE($2,lat), lng=COALESCE($3,lng), \
                    loc_accuracy_m=COALESCE($4,loc_accuracy_m), \
                    last_ingested_at = GREATEST(last_ingested_at, now()) \
             WHERE id=$1",
        )
        .bind(id)
        .bind(req.lat)
        .bind(req.lng)
        .bind(req.loc_accuracy_m)
        .execute(&mut *ctx.tx)
        .await?;
    }

    // Emit an idea.update sync op for multi-device parity (entity_ref = idea id, matching create).
    // Carry `body` in the op ONLY when it actually changed: a location-only edit emits `body:null`, so a
    // replaying device's materializer takes the location-only branch (applies location + bumps
    // last_ingested_at) instead of the `body <> $2` branch (which would update ZERO rows — silently
    // dropping the co-sent location and skipping the no-reingest guard). This matches THIS handler's own
    // location-only UPDATE above, so the two devices' `ideas` rows converge.
    let op_body: Option<String> = if body_changed { new_body.clone() } else { None };
    crate::sync::emit::emit_op(
        &mut ctx.tx,
        ctx.tenant_id,
        ctx.user_id,
        "idea",
        &id.to_string(),
        serde_json::json!({ "update": {
            "body": op_body,
            "lat": req.lat, "lng": req.lng, "loc_accuracy_m": req.loc_accuracy_m,
        } }),
    )
    .await?;
    ctx.tx.commit().await?;
    Ok(Json(serde_json::json!({ "id": id, "reingest": body_changed })))
}

/// DELETE /v1/captures/{id} — soft-delete a capture and remove its wiki contribution (CAP-R3, C5).
/// Effective immediately + reversible (audit rows): the 1:1 source page is soft-deleted, shared pages
/// drop this idea from their provenance and are soft-deleted only if left with no sources. Merged prose
/// in still-sourced pages stays until Auto-Dream reconciles. One tx (idea + page writes atomic).
async fn delete_capture(
    mut ctx: TenantCtx,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let existed: Option<bool> =
        sqlx::query_scalar("UPDATE ideas SET active=false WHERE id=$1 AND active RETURNING true")
            .bind(id)
            .fetch_optional(&mut *ctx.tx)
            .await?;
    if existed.is_none() {
        ctx.tx.commit().await?;
        return Err(ApiError::NotFound);
    }

    // Every non-deleted page this idea contributed to (GIN reverse-lookup on source_ids).
    let pages = sqlx::query(
        "SELECT id, type::text AS ty, source_ids FROM wiki_pages \
         WHERE deleted_at IS NULL AND source_ids @> ARRAY[$1]::uuid[]",
    )
    .bind(id)
    .fetch_all(&mut *ctx.tx)
    .await?;

    for p in &pages {
        let pid: Uuid = p.get("id");
        let ty: String = p.get("ty");
        let srcs: Vec<Uuid> = p.get("source_ids");
        let remaining: Vec<Uuid> = srcs.into_iter().filter(|s| *s != id).collect();
        // A `source` page is 1:1 with the capture → always soft-delete. A shared page → soft-delete only
        // if no sources remain; otherwise just drop the id from provenance (prose can't be un-merged).
        let orphan = remaining.is_empty();
        if ty == "source" || orphan {
            sqlx::query("UPDATE wiki_pages SET deleted_at=now() WHERE id=$1 AND deleted_at IS NULL")
                .bind(pid)
                .execute(&mut *ctx.tx)
                .await?;
            sqlx::query(
                "INSERT INTO approvals (tenant_id, user_id, action, subject_ref, requested_by) \
                 VALUES ($1,$2,'wiki_delete',$3,'user')",
            )
            .bind(ctx.tenant_id)
            .bind(ctx.user_id)
            .bind(serde_json::json!({ "page": pid.to_string() }))
            .execute(&mut *ctx.tx)
            .await?;
        } else {
            sqlx::query(
                "UPDATE wiki_pages SET source_ids = array_remove(source_ids, $2), \
                        sync_version = sync_version + 1, updated = now() WHERE id=$1",
            )
            .bind(pid)
            .bind(id)
            .execute(&mut *ctx.tx)
            .await?;
        }
    }

    // Emit an idea.delete sync op so other devices drop this capture on their next incremental pull.
    crate::sync::emit::emit_op(
        &mut ctx.tx,
        ctx.tenant_id,
        ctx.user_id,
        "idea",
        &id.to_string(),
        serde_json::json!({ "delete": {} }),
    )
    .await?;
    ctx.tx.commit().await?;
    Ok(Json(serde_json::json!({ "id": id, "deleted": true })))
}

/// Parse the optional `[start,end)` UTC range; both bounds must be present and valid to scope the feed.
fn parse_range(
    q: &FeedQuery,
) -> Result<Option<UtcRange>, ApiError> {
    let parse = |s: &str| {
        chrono::DateTime::parse_from_rfc3339(s)
            .map(|d| d.with_timezone(&chrono::Utc))
            .map_err(|_| ApiError::BadRequest(format!("invalid RFC3339 instant: {s}")))
    };
    match (q.start.as_deref(), q.end.as_deref()) {
        (Some(s), Some(e)) => Ok(Some((parse(s)?, parse(e)?))),
        (None, None) => Ok(None),
        _ => Err(ApiError::BadRequest("start and end must be provided together".into())),
    }
}

/// The capture feed rows for the caller's tenant (RLS-scoped via the bound GUC). A date-scoped
/// `[start,end)` window returns ALL of that window newest-first (lifting the 50-cap to a safety ceiling —
/// a single day is bounded), otherwise the default newest-50. The `ideas_feed_idx (tenant_id, user_id,
/// captured_at DESC)` index covers both. `tx` must already have `app.tenant_id` bound.
pub async fn query_feed(
    tx: &mut sqlx::PgConnection,
    range: Option<UtcRange>,
) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    let rows = match range {
        Some((start, end)) => {
            sqlx::query(
                "SELECT id, kind::text AS kind, body, ingest_state::text AS st, captured_at \
                 FROM ideas WHERE active AND captured_at >= $1 AND captured_at < $2 \
                 ORDER BY captured_at DESC LIMIT 500",
            )
            .bind(start)
            .bind(end)
            .fetch_all(&mut *tx)
            .await?
        }
        None => {
            sqlx::query(
                "SELECT id, kind::text AS kind, body, ingest_state::text AS st, captured_at \
                 FROM ideas WHERE active ORDER BY captured_at DESC LIMIT 50",
            )
            .fetch_all(&mut *tx)
            .await?
        }
    };
    Ok(rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "kind": r.get::<String, _>("kind"),
                "body": r.get::<String, _>("body"),
                "ingest_state": r.get::<String, _>("st"),
                "captured_at": r.get::<chrono::DateTime<chrono::Utc>, _>("captured_at"),
            })
        })
        .collect())
}
