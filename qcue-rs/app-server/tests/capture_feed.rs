// QCue: the capture feed can be scoped to a [start,end) UTC window so the app's calendar/date picker can
// show ALL captures of a chosen local day — not just whatever falls inside the newest-50 default feed.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;

use app_server::capture::routes::query_feed;
use sqlx::PgPool;
use uuid::Uuid;

async fn insert_idea(db: &TestDb, tenant: Uuid, user: Uuid, body: &str, captured_at: &str) {
    let mut tx = tenant_tx(db, tenant).await;
    sqlx::query(
        "INSERT INTO ideas(tenant_id,user_id,body,captured_at,log_ref) \
         VALUES ($1,$2,$3,$4::timestamptz,$5)",
    )
    .bind(tenant)
    .bind(user)
    .bind(body)
    .bind(captured_at)
    .bind(format!("2026/test/{body}.jsonl#1"))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

fn utc(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&chrono::Utc)
}

#[sqlx::test(migrations = "../migrations")]
async fn feed_scoped_to_a_day_returns_only_that_days_captures(pool: PgPool) {
    let db = from_pool(pool);
    let (tid, uid) = seed_tenant(&db, "feed-day").await;
    insert_idea(&db, tid, uid, "yesterday", "2026-06-14T23:30:00Z").await;
    insert_idea(&db, tid, uid, "morning", "2026-06-15T08:00:00Z").await;
    insert_idea(&db, tid, uid, "evening", "2026-06-15T22:00:00Z").await;
    insert_idea(&db, tid, uid, "next-day", "2026-06-16T01:00:00Z").await;

    let mut tx = tenant_tx(&db, tid).await;
    // The UTC [start,end) window for the chosen day 2026-06-15.
    let scoped = query_feed(&mut tx, Some((utc("2026-06-15T00:00:00Z"), utc("2026-06-16T00:00:00Z"))))
        .await
        .unwrap();
    let all = query_feed(&mut tx, None).await.unwrap();
    tx.commit().await.unwrap();

    let bodies: Vec<String> =
        scoped.iter().map(|v| v["body"].as_str().unwrap().to_string()).collect();
    assert_eq!(bodies, vec!["evening", "morning"], "only the chosen day, newest-first");
    assert_eq!(all.len(), 4, "an unscoped feed still returns everything (within the cap)");
}
