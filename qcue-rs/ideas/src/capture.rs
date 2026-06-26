// QCue S2-R9/S2-R51 / Master §3 persistence law — persist the ideas row FIRST, then enqueue the ingest
// job. A crash after the row lands still records intent (`ingest_state='pending'` → a reaper
// re-enqueues). The voice path consumes the S1 transcript (the STT envelope already produced the text;
// D4) and routes it through the SAME conversation-ingest as text.
//
// NOTE: the live HTTP capture surface (`app-server::capture`) already implements the object-store
// JSONL-first + tenant-GUC-tx + enqueue flow; this is the library-level capture entry the ideas crate
// exposes for non-HTTP callers (and the voice path), keeping the persist-before-enqueue invariant.
use sqlx::PgPool;
use uuid::Uuid;

pub enum CaptureKind {
    Text { body: String },
    /// S1 TranscriptionProvider already produced the transcript (D4); ingest treats it as a source.
    Voice { transcript: String, provider: String },
    Clip { caption: String, url: String },
}

pub struct CaptureInput {
    pub kind: CaptureKind,
    pub origin: String,
}

pub struct CaptureResult {
    pub idea_id: Uuid,
    pub ingest_job_id: Option<Uuid>,
}

/// Persist BEFORE any LLM call (a crash mid-ingest still records intent). All DML runs inside one
/// tenant-GUC tx so FORCE RLS (B-R4/B-R5) scopes the writes; the row + the enqueue commit atomically.
pub async fn ingest_capture(
    tenant: Uuid,
    user: Uuid,
    input: CaptureInput,
    pool: &PgPool,
) -> anyhow::Result<CaptureResult> {
    let (kind, body, provider, url) = match input.kind {
        CaptureKind::Text { body } => ("text", body, None, None),
        CaptureKind::Voice { transcript, provider } => ("voice", transcript, Some(provider), None),
        CaptureKind::Clip { caption, url } => ("clip", caption, None, Some(url)),
    };
    let idea_id = Uuid::now_v7();
    // canonical JSONL log_ref (the object-store key) — written by the background writer; ref computed here.
    let log_ref = format!("captures/{idea_id}.jsonl");

    let mut tx = pool.begin().await?;
    set_tenant(&mut tx, tenant).await?;
    sqlx::query(
        "INSERT INTO ideas (id, tenant_id, user_id, kind, body, source_url, log_ref, transcript_provider, origin, ingest_state) \
         VALUES ($1,$2,$3,$4::idea_kind,$5,$6,$7,$8,$9,'pending')",
    )
    .bind(idea_id)
    .bind(tenant)
    .bind(user)
    .bind(kind)
    .bind(&body)
    .bind(url)
    .bind(&log_ref)
    .bind(provider)
    .bind(&input.origin)
    .execute(&mut *tx)
    .await?;
    // enqueue an ingest job (S3 workers claim it; S2 supplies the handler).
    let job_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO jobs (id, tenant_id, user_id, kind, payload) \
         VALUES ($1,$2,$3,'ingest', jsonb_build_object('idea_id', $4::text))",
    )
    .bind(job_id)
    .bind(tenant)
    .bind(user)
    .bind(idea_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE ideas SET ingest_job_id=$2 WHERE id=$1")
        .bind(idea_id)
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(CaptureResult { idea_id, ingest_job_id: Some(job_id) })
}

async fn set_tenant(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant: Uuid,
) -> anyhow::Result<()> {
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
