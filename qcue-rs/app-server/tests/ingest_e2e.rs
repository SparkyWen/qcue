// QCue S2 — end-to-end capture→ingest→wiki-page loop. Enqueue a `kind='ingest'` job for a persisted
// idea, register the real IngestHandler (backed by a scripted StubWikiLlm — keyless, networkless), run
// the worker once, then assert the wiki pages were created through the write-gate and the idea
// transitioned to `ingested`. This is the loop the live `POST /v1/capture` + worker pool drive.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::ingest::IngestHandler;
use app_server::jobs::queue::{enqueue, JobKind};
use app_server::jobs::worker::{run_once_registry, HandlerRegistry, WorkerGates};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;
use wiki::llm::StubWikiLlm;

#[sqlx::test(migrations = "../migrations")]
async fn capture_to_page_via_worker(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "ingest-e2e").await;

    // 1) persist an idea row (the capture surface does this first; here we seed it directly).
    let idea_id = Uuid::now_v7();
    {
        let mut tx = tenant_tx(&db, tid).await;
        sqlx::query(
            "INSERT INTO ideas(id,tenant_id,user_id,kind,body,log_ref,origin,ingest_state) \
             VALUES ($1,$2,$3,'text','Notes about Tokio async runtime',$4,'capture','pending')",
        )
        .bind(idea_id)
        .bind(tid)
        .bind(uid)
        .bind(format!("captures/{idea_id}.jsonl"))
        .execute(&mut *tx)
        .await
        .unwrap();
        // 2) enqueue the kind='ingest' job carrying the idea id (what /v1/capture does).
        let _ = enqueue(
            &mut tx,
            tid,
            Some(uid),
            JobKind::Ingest,
            serde_json::json!({ "idea_id": idea_id.to_string() }),
            Some(&format!("ingest:{idea_id}")),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }

    // 3) build the ingest handler with a scripted WikiLlm (dedup→extract→summary→entity) and register
    //    it for kind='ingest' (Echo stays the default for unknown kinds).
    let vault = tempfile::tempdir().unwrap();
    let llm = Arc::new(StubWikiLlm::scripted(vec![
        r#"{"fully_redundant":false}"#.into(),
        r#"{"source_title":"Tokio notes","summary":"async runtime notes","entities":[{"name":"Tokio","aliases":[]}],"concepts":[],"contradictions":[],"related_pages":[],"key_points":[]}"#.into(),
        "Summary page body linking [[tokio]].".into(),
        "Tokio is an async runtime.".into(),
    ]));
    let handler: Arc<dyn app_server::jobs::worker::JobHandler> =
        Arc::new(IngestHandler::new(db.app.clone(), vault.path().to_path_buf(), llm));
    let registry = HandlerRegistry::new().with_ingest(handler);

    // 4) run the worker once for the ingest family (gated ON).
    let gates = WorkerGates { ingest: true, lint: false, dream: false, sync: false };
    let n = run_once_registry(&db.app, &gates, tid, JobKind::Ingest, "w0", &registry)
        .await
        .unwrap();
    assert_eq!(n, 1, "the ingest job was claimed and run");

    // 5) the job is done and its result is the IngestReport.
    let (state, result): (String, serde_json::Value) = {
        let mut tx = tenant_tx(&db, tid).await;
        let r = sqlx::query_as("SELECT state::text, result FROM jobs WHERE tenant_id=$1 AND kind='ingest'")
            .bind(tid)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        r
    };
    assert_eq!(state, "done");
    assert!(result.get("created_pages").is_some(), "result is the IngestReport: {result}");

    // 6) wiki pages exist (created through the write-gate): a semantic source page + the tokio entity.
    let pages: Vec<(String, String)> = {
        let mut tx = tenant_tx(&db, tid).await;
        let rows = sqlx::query_as(
            "SELECT slug, type::text FROM wiki_pages WHERE tenant_id=$1 AND deleted_at IS NULL",
        )
        .bind(tid)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        rows
    };
    assert!(
        pages.iter().any(|(s, t)| s == "tokio-notes" && t == "source"),
        "semantic source page created: {pages:?}"
    );
    assert!(
        pages.iter().any(|(s, t)| s == "tokio" && t == "entity"),
        "tokio entity page created: {pages:?}"
    );

    // 7) the idea transitioned to ingested.
    let ingest_state: String = {
        let mut tx = tenant_tx(&db, tid).await;
        let s: (String,) = sqlx::query_as("SELECT ingest_state::text FROM ideas WHERE id=$1")
            .bind(idea_id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        s.0
    };
    assert_eq!(ingest_state, "ingested");
}
