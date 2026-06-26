//! QCue S3-R49/R50 + SYNC-D1/D3 — apply unapplied `sync_ops` in total HLC order into the CANONICAL
//! tables (`ideas`, and `wiki_pages` via `write_gate`), and serve the pull-since cursor.
//!
//! The reducer is deterministic + commutative under the `(wall_ms, lamport, site_id)` total order, so
//! two devices that pushed the same op-set materialize identical state (B-R21 / D6). Materialization is
//! the ONLY writer into `ideas`/`wiki_pages` for synced fields (spec §7). `idea.create` is an
//! idempotent insert that reuses the capture-insert columns (`log_ref` via the object store, `kind`,
//! `origin`); idempotency is by `idempotency_key = entity_ref` (the client uuid). Wiki ops flow through
//! `wiki::write_gate::write_page` — the single body-write site (SYNC-D3) — which sanitizes
//! `[[wikilinks]]`, rebuilds `wiki_links`, writes the `.md`, and sets `content_hash`/`sync_version`.
//! Re-applying an op is a no-op (`applied` is flipped; the wiki upsert + idea insert are idempotent).
use crate::objstore::ObjStore;
use crate::tenancy::TenantTx;
use protocol::sync::{IdeaSnap, SyncOp, SyncSnapshot, WikiPageSnap};
use sqlx::Row;
use uuid::Uuid;
use wiki::write_gate::{PageWrite, WikiWriteGate};

/// Apply all unapplied ops for a tenant in `(wall_ms, lamport, site_id)` order into the canonical
/// tables (SYNC-D1). Returns how many ops were applied. Idempotent: an already-applied op is skipped by
/// the `NOT applied` predicate, and re-running over the same set applies nothing.
///
/// `user_id` owns any `idea` row materialized here; `objstore` writes the canonical JSONL line
/// (`ideas.log_ref` is NOT NULL — the object-store key, §9). `gate` is the tenant-scoped wiki
/// write-gate (constructed with the tenant vault root) through which every wiki body write flows.
pub async fn apply_unapplied(
    tx: &mut TenantTx,
    tenant_id: Uuid,
    user_id: Uuid,
    objstore: &ObjStore,
    gate: &WikiWriteGate,
) -> sqlx::Result<u64> {
    let rows = sqlx::query(
        "SELECT id, entity_kind, entity_ref, op FROM sync_ops WHERE tenant_id=$1 AND NOT applied \
         ORDER BY hlc_wall_ms, hlc_lamport, site_id",
    )
    .bind(tenant_id)
    .fetch_all(&mut **tx)
    .await?;
    let mut applied = 0u64;
    for r in &rows {
        let entity_kind: String = r.get("entity_kind");
        let entity_ref: String = r.get("entity_ref");
        let op: serde_json::Value = r.get("op");
        match entity_kind.as_str() {
            // SYNC-D1 §5: `idea.create` → idempotent INSERT into `ideas` (no-op if the client uuid
            // already landed). entity_ref is the client uuid / idempotency key.
            "idea" => {
                if let Some(create) = op.get("create") {
                    apply_idea_create(tx, tenant_id, user_id, objstore, &entity_ref, create).await?;
                } else if let Some(update) = op.get("update") {
                    apply_idea_update(tx, tenant_id, &entity_ref, update).await?;
                } else if op.get("delete").is_some() {
                    apply_idea_delete(tx, tenant_id, &entity_ref).await?;
                }
            }
            // SYNC-D3 §5: `wiki_page` create/set_title/set_body/delete → through write_gate (the single
            // body-write site) into wiki_pages + wiki_links. entity_ref is the slug.
            "wiki_page" => {
                apply_wiki_op(tx, tenant_id, gate, &entity_ref, &op).await?;
            }
            _ => {}
        }
        sqlx::query("UPDATE sync_ops SET applied=true WHERE id=$1")
            .bind(r.get::<Uuid, _>("id"))
            .execute(&mut **tx)
            .await?;
        applied += 1;
    }
    Ok(applied)
}

/// Apply one `wiki_page` op (§5) through the write-gate. Reads the current type/title and body (so a
/// title-only or body-only op preserves the other field), merges the op's changes, then calls
/// `write_page` (sanitizes links, rebuilds wiki_links, writes the .md, sets content_hash + bumps
/// sync_version). `delete` soft-deletes the row. Unknown op keys are ignored (forward-compat).
async fn apply_wiki_op(
    tx: &mut TenantTx,
    tenant_id: Uuid,
    gate: &WikiWriteGate,
    slug: &str,
    op: &serde_json::Value,
) -> sqlx::Result<()> {
    // delete → soft-delete (tombstone). Find the live page id for the slug.
    if op.get("delete").and_then(|v| v.as_bool()).unwrap_or(false) {
        if let Some(id) = live_page_id(tx, tenant_id, slug).await? {
            sqlx::query("UPDATE wiki_pages SET deleted_at=now() WHERE tenant_id=$1 AND id=$2 AND deleted_at IS NULL")
                .bind(tenant_id)
                .bind(id)
                .execute(&mut **tx)
                .await?;
        }
        return Ok(());
    }

    // read the current row (type/title/id) for the slug, if any, to preserve unset fields.
    let current: Option<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, type::text, title FROM wiki_pages \
         WHERE tenant_id=$1 AND slug=$2 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(tenant_id)
    .bind(slug)
    .fetch_optional(&mut **tx)
    .await?;

    // determine the page type: from the op's create.type, else the existing row, else default concept.
    let r#type = op
        .get("create")
        .and_then(|c| c.get("type"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| current.as_ref().map(|(_, t, _)| t.clone()))
        .unwrap_or_else(|| "concept".to_string());

    // title: set_title wins; else keep the existing title; else fall back to the slug.
    let title = op
        .get("set_title")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| current.as_ref().map(|(_, _, t)| t.clone()))
        .unwrap_or_else(|| slug.to_string());

    // body: set_body wins; else preserve the current body off disk; else empty (a bare create).
    let body = match op.get("set_body").and_then(|v| v.as_str()) {
        Some(b) => b.to_string(),
        None => match &current {
            Some((id, _, _)) => gate.read_body(tenant_id, *id).await.unwrap_or_default(),
            None => String::new(),
        },
    };

    // If this op carries nothing that changes the page (no create/set_title/set_body), skip — a bare
    // unknown op shouldn't churn the page (forward-compat).
    let touches_page = op.get("create").is_some()
        || op.get("set_title").is_some()
        || op.get("set_body").is_some();
    if !touches_page {
        return Ok(());
    }

    gate.write_page(
        tenant_id,
        PageWrite {
            r#type,
            slug: slug.to_string(),
            title,
            aliases: vec![],
            tags: vec![],
            summary: String::new(),
            source_ids: vec![],
            body,
            llm_created: None,
            llm_reviewed: None,
        },
    )
    .await
    .map_err(|e| sqlx::Error::Io(std::io::Error::other(e.to_string())))?;
    Ok(())
}

/// The live (non-deleted) page id for a slug, if any.
async fn live_page_id(tx: &mut TenantTx, tenant_id: Uuid, slug: &str) -> sqlx::Result<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM wiki_pages WHERE tenant_id=$1 AND slug=$2 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(tenant_id)
    .bind(slug)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|r| r.0))
}

/// Idempotent-insert one materialized capture into `ideas` (SYNC-D1 §5). Mirrors the capture-insert
/// site (`capture/routes.rs`): persists the canonical JSONL line FIRST (object store, B-R29) so
/// `log_ref` is set, then inserts the row with `ON CONFLICT (tenant_id, idempotency_key) DO NOTHING`
/// keyed on `entity_ref` (the client uuid) so a re-applied / re-pushed op never duplicates a row.
async fn apply_idea_create(
    tx: &mut TenantTx,
    tenant_id: Uuid,
    user_id: Uuid,
    objstore: &ObjStore,
    entity_ref: &str,
    create: &serde_json::Value,
) -> sqlx::Result<()> {
    // NOTE on tag-escaping: idea bodies that reach this materializer were ALREADY escaped
    // (`escape_reserved_tags`) at the originating capture HTTP endpoint, whose `idea.create` emit
    // carries that escaped body. Sync is PULL-ONLY today (no client idea-push), so no body arrives here
    // un-escaped and we do NOT re-escape (double-escaping would corrupt the content). If a client
    // idea-PUSH path is ever added, it MUST escape on the way in (here or at push) before this insert.
    let body = create.get("body").and_then(|v| v.as_str()).unwrap_or("");
    let origin = create.get("origin").and_then(|v| v.as_str()).unwrap_or("capture");
    let captured_at = create.get("captured_at").and_then(|v| v.as_str());

    let idea_id = Uuid::now_v7();
    // Persist the canonical JSONL line first (the object store redacts; B-R11/B-R29). A failure here
    // surfaces as an I/O error mapped into sqlx::Error so the whole apply tx rolls back.
    let log_ref = objstore
        .append_capture(
            tenant_id,
            user_id,
            idea_id,
            &serde_json::json!({ "kind": "text", "body": body, "origin": origin }),
        )
        .map_err(|e| sqlx::Error::Io(std::io::Error::other(e)))?;

    // Parse the client-supplied captured_at (RFC3339) if present; else default to now() via COALESCE so
    // the two devices agree on the capture time when the client provided one.
    let parsed_at: Option<chrono::DateTime<chrono::Utc>> = captured_at
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&chrono::Utc)));

    sqlx::query(
        "INSERT INTO ideas(id,tenant_id,user_id,kind,body,log_ref,origin,idempotency_key,captured_at) \
         VALUES ($1,$2,$3,'text'::idea_kind,$4,$5,$6,$7,COALESCE($8, now())) \
         ON CONFLICT (tenant_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING",
    )
    .bind(idea_id)
    .bind(tenant_id)
    .bind(user_id)
    .bind(body)
    .bind(&log_ref)
    .bind(origin)
    .bind(entity_ref)
    .bind(parsed_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// SYNC — apply an `idea.update` (CAP-R2 parity with the HTTP PATCH). Locate the row by its
/// cross-device key (`idempotency_key = entity_ref`, the origin idea ref bound by `apply_idea_create`)
/// and apply the same content-compare body rule as `capture::routes::edit`. A body change → normal
/// UPDATE guarded by `body <> $2` (idempotent + the touch trigger dirties the row for re-ingest). A
/// location-only update keeps `last_ingested_at = GREATEST(last_ingested_at, now())` so `updated_at`
/// never outruns it and the dirty-scan stays false — no re-ingest, no token spend.
async fn apply_idea_update(
    tx: &mut TenantTx,
    tenant_id: Uuid,
    entity_ref: &str,
    update: &serde_json::Value,
) -> sqlx::Result<()> {
    // NOTE on tag-escaping: like `apply_idea_create`, a `body` arriving here was ALREADY escaped at the
    // originating edit HTTP endpoint (the emit carries the escaped body, and only on an actual change —
    // a location-only edit emits `body:null`, taking the location-only branch below). Sync is pull-only
    // today (no client idea-push), so no body arrives un-escaped and we do NOT re-escape here. A client
    // idea-PUSH path would need to escape before this UPDATE.
    let new_body = update.get("body").and_then(|v| v.as_str());
    let lat = update.get("lat").and_then(|v| v.as_f64());
    let lng = update.get("lng").and_then(|v| v.as_f64());
    let acc = update.get("loc_accuracy_m").and_then(|v| v.as_f64()).map(|v| v as f32);
    if let Some(b) = new_body {
        sqlx::query(
            "UPDATE ideas SET body=$2, lat=COALESCE($3,lat), lng=COALESCE($4,lng), loc_accuracy_m=COALESCE($5,loc_accuracy_m) \
             WHERE tenant_id=$1 AND idempotency_key=$6 AND body <> $2",
        )
        .bind(tenant_id)
        .bind(b)
        .bind(lat)
        .bind(lng)
        .bind(acc)
        .bind(entity_ref)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query(
            "UPDATE ideas SET lat=COALESCE($2,lat), lng=COALESCE($3,lng), loc_accuracy_m=COALESCE($4,loc_accuracy_m), \
                    last_ingested_at=GREATEST(last_ingested_at, now()) \
             WHERE tenant_id=$1 AND idempotency_key=$5",
        )
        .bind(tenant_id)
        .bind(lat)
        .bind(lng)
        .bind(acc)
        .bind(entity_ref)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// SYNC — apply an `idea.delete` (CAP-R3 parity with the HTTP DELETE): soft-delete by the cross-device
/// key. The wiki cascade is the originating device's responsibility (it ran the HTTP DELETE); the
/// receiving device only hides the row from the feed.
async fn apply_idea_delete(tx: &mut TenantTx, tenant_id: Uuid, entity_ref: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE ideas SET active=false WHERE tenant_id=$1 AND idempotency_key=$2")
        .bind(tenant_id)
        .bind(entity_ref)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Read the materialized wiki-page state (title + content_hash + sync_version) for a slug from the
/// CANONICAL `wiki_pages` table (SYNC-D3 — replaces the legacy `session_kv` projection). Returns
/// `None` if the slug has no live page. The body lives in the vault (read via the gate); callers that
/// need it use `WikiWriteGate::read_body`.
pub async fn wiki_page_state(
    tx: &mut TenantTx,
    tenant_id: Uuid,
    slug: &str,
) -> sqlx::Result<Option<serde_json::Value>> {
    let row: Option<(String, Option<String>, i64)> = sqlx::query_as(
        "SELECT title, content_hash, sync_version FROM wiki_pages \
         WHERE tenant_id=$1 AND slug=$2 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(tenant_id)
    .bind(slug)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|(title, content_hash, sync_version)| {
        serde_json::json!({
            "title": title,
            "content_hash": content_hash,
            "sync_version": sync_version,
        })
    }))
}

/// Pull the ops with `hlc_wall_ms > since`, HLC-ordered. LEGACY (the lossy wall-ms cursor): retained
/// only for the unit tests that exercise it directly. The HTTP pull uses the gap-free `seq` cursor via
/// [`ops_since_seq`] (SYNC-D4) — two ops sharing a wall-ms could be missed/re-sent by this one.
pub async fn ops_since(
    tx: &mut TenantTx,
    tenant_id: Uuid,
    since_wall_ms: i64,
) -> sqlx::Result<Vec<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT op FROM sync_ops WHERE tenant_id=$1 AND hlc_wall_ms > $2 ORDER BY hlc_wall_ms, hlc_lamport, site_id",
    )
    .bind(tenant_id)
    .bind(since_wall_ms)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.iter().map(|r| r.get::<serde_json::Value, _>("op")).collect())
}

/// The current pull cursor for a tenant: `MAX(seq)`, or 0 on an empty op-log (SYNC-D4). A pull returns
/// this so the next incremental pull resumes after everything currently logged (computed BEFORE the ops
/// read, so a concurrent insert at worst re-sends an op next pull — never silently skips one).
pub async fn tenant_cursor(tx: &mut TenantTx, tenant_id: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COALESCE(MAX(seq),0) FROM sync_ops WHERE tenant_id=$1")
        .bind(tenant_id)
        .fetch_one(&mut **tx)
        .await
}

/// The ops with `seq > since`, in `seq` order — the gap-free incremental change feed (SYNC-D4). Returns
/// the full `SyncOp` (HLC tuple + entity + op bag) so a warm device applies each op through its reducer.
pub async fn ops_since_seq(tx: &mut TenantTx, tenant_id: Uuid, since: i64) -> sqlx::Result<Vec<SyncOp>> {
    let rows = sqlx::query(
        "SELECT hlc_wall_ms, hlc_lamport, site_id, entity_kind, entity_ref, op FROM sync_ops \
         WHERE tenant_id=$1 AND seq > $2 ORDER BY seq",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .iter()
        .map(|r| SyncOp {
            hlc_wall_ms: r.get("hlc_wall_ms"),
            hlc_lamport: r.get("hlc_lamport"),
            site_id: r.get("site_id"),
            entity_kind: r.get("entity_kind"),
            entity_ref: r.get("entity_ref"),
            op: r.get("op"),
        })
        .collect())
}

/// The cold-start snapshot of the canonical tables (SYNC-D5): every live capture + every live wiki page
/// (bodies omitted — fetched by `content_hash` if the client lacks it, SYNC-D6). Read straight from the
/// canonical tables, so it reflects rows that predate the op-log and any server-origin rows.
pub async fn snapshot(tx: &mut TenantTx, tenant_id: Uuid) -> sqlx::Result<SyncSnapshot> {
    let irows = sqlx::query(
        "SELECT id, body, COALESCE(origin,'capture') AS origin, captured_at FROM ideas \
         WHERE tenant_id=$1 AND active ORDER BY captured_at",
    )
    .bind(tenant_id)
    .fetch_all(&mut **tx)
    .await?;
    let ideas = irows
        .iter()
        .map(|r| IdeaSnap {
            id: r.get::<Uuid, _>("id").to_string(),
            body: r.get("body"),
            origin: r.get("origin"),
            captured_at: r.get::<chrono::DateTime<chrono::Utc>, _>("captured_at").to_rfc3339(),
        })
        .collect();
    let wrows = sqlx::query(
        "SELECT slug, title, content_hash, sync_version FROM wiki_pages \
         WHERE tenant_id=$1 AND deleted_at IS NULL ORDER BY slug",
    )
    .bind(tenant_id)
    .fetch_all(&mut **tx)
    .await?;
    let wiki_pages = wrows
        .iter()
        .map(|r| WikiPageSnap {
            slug: r.get("slug"),
            title: r.get("title"),
            content_hash: r.get::<Option<String>, _>("content_hash").unwrap_or_default(),
            sync_version: r.get("sync_version"),
        })
        .collect();
    Ok(SyncSnapshot { ideas, wiki_pages })
}
