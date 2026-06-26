// QCue DIG-R2 — the dirty scan returns 'pending' + edited-since-ingest ideas, excludes ingested-and-unchanged.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use sqlx::PgPool;
use store::ideas_repo::IdeasRepo;
use uuid::Uuid;

async fn set_tenant(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, t: Uuid) {
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(t.to_string())
        .execute(&mut **tx)
        .await
        .unwrap();
}

async fn seed_tenant_user(pool: &PgPool, t: Uuid) -> Uuid {
    sqlx::query("INSERT INTO tenants (id, slug, display_name, namespace) VALUES ($1,$2,$2,$3)")
        .bind(t)
        .bind(format!("t-{t}"))
        .bind(format!("t/{t}"))
        .execute(pool)
        .await
        .unwrap();
    let uid = Uuid::now_v7();
    let mut tx = pool.begin().await.unwrap();
    set_tenant(&mut tx, t).await;
    sqlx::query("INSERT INTO users (id, tenant_id, email) VALUES ($1,$2,$3)")
        .bind(uid)
        .bind(t)
        .bind(format!("u-{t}@x.test"))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    uid
}

/// Insert an idea with an explicit ingest_state + last_ingested_at, returning its id. When `bump_after`
/// is true, a follow-up UPDATE to body lifts updated_at PAST last_ingested_at (simulating an edit).
async fn seed_idea(
    pool: &PgPool,
    t: Uuid,
    uid: Uuid,
    state: &str,
    last_ingested: bool,
    bump_after: bool,
) -> Uuid {
    let id = Uuid::now_v7();
    let mut tx = pool.begin().await.unwrap();
    set_tenant(&mut tx, t).await;
    sqlx::query(
        "INSERT INTO ideas (id, tenant_id, user_id, kind, body, log_ref, origin, ingest_state, last_ingested_at) \
         VALUES ($1,$2,$3,'text','original',$4,'capture',$5::ingest_state, \
                 CASE WHEN $6 THEN now() ELSE NULL END)",
    )
    .bind(id)
    .bind(t)
    .bind(uid)
    .bind(format!("captures/{id}.jsonl"))
    .bind(state)
    .bind(last_ingested)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    if bump_after {
        // a later edit: the ideas_touch trigger bumps updated_at > last_ingested_at.
        let mut tx = pool.begin().await.unwrap();
        set_tenant(&mut tx, t).await;
        sqlx::query("UPDATE ideas SET body='edited' WHERE id=$1").bind(id).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    id
}

#[sqlx::test(migrations = "../migrations")]
async fn dirty_scan_returns_pending_and_edited_not_unchanged(pool: PgPool) {
    let t = Uuid::now_v7();
    let uid = seed_tenant_user(&pool, t).await;
    let repo = IdeasRepo::new(pool.clone());

    // (a) pending, never ingested → DIRTY
    let pending = seed_idea(&pool, t, uid, "pending", false, false).await;
    // (b) ingested + edited-since (updated_at > last_ingested_at) → DIRTY
    let edited = seed_idea(&pool, t, uid, "ingested", true, true).await;
    // (c) ingested + unchanged (last_ingested_at IS NOT NULL, no later edit) → NOT dirty
    let _clean = seed_idea(&pool, t, uid, "ingested", true, false).await;

    let dirty = repo.select_dirty_for_ingest(t).await.unwrap();
    assert!(dirty.contains(&pending), "pending idea is dirty: {dirty:?}");
    assert!(dirty.contains(&edited), "edited-since-ingest idea is dirty: {dirty:?}");
    assert_eq!(dirty.len(), 2, "ingested-and-unchanged is excluded: {dirty:?}");
}

#[sqlx::test(migrations = "../migrations")]
async fn set_last_ingested_excludes_idea_from_dirty(pool: PgPool) {
    let t = Uuid::now_v7();
    let uid = seed_tenant_user(&pool, t).await;
    let repo = IdeasRepo::new(pool.clone());
    let id = seed_idea(&pool, t, uid, "ingested", true, true).await; // edited → dirty
    assert!(repo.select_dirty_for_ingest(t).await.unwrap().contains(&id));
    // marking it freshly ingested lifts last_ingested_at >= updated_at → no longer dirty.
    repo.set_last_ingested(t, id).await.unwrap();
    assert!(!repo.select_dirty_for_ingest(t).await.unwrap().contains(&id));
}
