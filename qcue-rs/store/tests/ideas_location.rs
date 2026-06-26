// QCue LOC-R1 — verify that the ideas table has nullable lat/lng/loc_accuracy_m columns.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use sqlx::PgPool;
use uuid::Uuid;

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
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(t.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
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

#[sqlx::test(migrations = "../migrations")]
async fn test_ideas_has_location_columns(pool: PgPool) {
    // Insert a row carrying location and read it back — fails until the migration adds the columns.
    let tid = Uuid::now_v7();
    let uid = seed_tenant_user(&pool, tid).await;

    let id = Uuid::now_v7();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
        .bind(tid.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO ideas(id,tenant_id,user_id,kind,body,log_ref,lat,lng,loc_accuracy_m) \
         VALUES ($1,$2,$3,'text'::idea_kind,'hi','captures/x.jsonl',31.23,121.47,12.5)",
    )
    .bind(id)
    .bind(tid)
    .bind(uid)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    use sqlx::Row;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
        .bind(tid.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let row = sqlx::query("SELECT lat,lng,loc_accuracy_m FROM ideas WHERE id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(row.get::<Option<f64>, _>("lat"), Some(31.23));
    assert_eq!(row.get::<Option<f32>, _>("loc_accuracy_m"), Some(12.5));
}
