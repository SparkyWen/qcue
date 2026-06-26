// QCue S2-R9 — voice path: STT transcript → ideas(kind=voice, origin=voice) → persist-before-enqueue.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
mod fixtures {
    include!("../../wiki/tests/fixtures/pg.rs");
}
use fixtures::{seed_two_tenants, TestDb};
use ideas::capture::{ingest_capture, CaptureInput, CaptureKind};
use sqlx::PgPool;

#[sqlx::test(migrations = "../migrations")]
async fn capture_persists_idea_before_enqueue_and_voice_routes_transcript(pool: PgPool) {
    let db = TestDb::new(pool);
    let (a, _) = seed_two_tenants(&db).await;
    let user = db.user_of(a).await;
    // a voice capture: a stub transcript stands in for STT (S1 TranscriptionProvider envelope).
    let res = ingest_capture(
        a,
        user,
        CaptureInput {
            kind: CaptureKind::Voice {
                transcript: "spoken note about Rust".into(),
                provider: "stub-stt".into(),
            },
            origin: "voice".into(),
        },
        &db.pool,
    )
    .await
    .unwrap();
    // the ideas row is persisted FIRST (before any ingest job), kind=voice + transcript_provider set.
    let row: (String, String, Option<String>, String) = {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(a.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let r = sqlx::query_as(
            "SELECT kind::text, body, transcript_provider, ingest_state::text FROM ideas WHERE id=$1",
        )
        .bind(res.idea_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        r
    };
    assert_eq!(row.0, "voice");
    assert_eq!(row.1, "spoken note about Rust");
    assert_eq!(row.2.as_deref(), Some("stub-stt"));
    assert_eq!(row.3, "pending"); // enqueued for ingest, not yet processed
    assert!(res.ingest_job_id.is_some()); // a jobs row was created
}
