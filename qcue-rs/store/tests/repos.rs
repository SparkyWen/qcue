#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R34,R56,R65 — Postgres source-of-truth + Redis cooldown/lease; cross-worker cooldown visibility.
use store::cooldown::Cooldown;
use store::creds_repo::CredsRepo;
use store::messages_repo::{ConversationsRepo, MessagesRepo};
use uuid::Uuid;

#[allow(dead_code)]
fn skip_without_db() -> bool {
    std::env::var("QCUE_TEST_DB").is_err()
}

// Migrations resolve from the SINGLE workspace dir `qcue-rs/migrations/` (M0 owned by S3, M1 by S1),
// which is `../migrations` relative to the `store` crate; there is no `store/migrations/`.
#[sqlx::test(migrations = "../migrations")]
async fn test_status_flip_persists_in_postgres(pool: sqlx::PgPool) {
    // set tenant GUC, insert a credential, flip status to 'exhausted', read it back.
    let repo = CredsRepo::new(pool.clone());
    let tenant = Uuid::now_v7();
    repo.set_tenant_guc(tenant).await.unwrap();
    let id = repo.insert_test_credential(tenant, "openai", "hint-1234").await.unwrap();
    repo.mark_exhausted(id, "openai").await.unwrap();
    let rows = repo.list(tenant, "openai").await.unwrap();
    assert_eq!(rows.iter().find(|r| r.id == id).unwrap().status, "exhausted");
}

#[sqlx::test(migrations = "../migrations")]
async fn test_message_persisted_before_call_is_durable(pool: sqlx::PgPool) {
    // S1-R56 — a user message inserted in the prologue is durable.
    let repo = MessagesRepo::new(pool.clone());
    let tenant = Uuid::now_v7();
    let user = Uuid::now_v7();
    let session = Uuid::now_v7();
    repo.set_tenant_guc(tenant).await.unwrap();
    repo.insert_user(tenant, user, session, "captured intent").await.unwrap();
    let msgs = repo.read_session(tenant, session).await.unwrap();
    assert!(msgs.iter().any(|m| m.content.as_deref() == Some("captured intent")));
}

#[sqlx::test(migrations = "../migrations")]
async fn test_conversations_table_is_rls_forced(pool: sqlx::PgPool) {
    // The conversations header table exists with FORCE ROW LEVEL SECURITY (REC-R2).
    let forced: (bool,) = sqlx::query_as(
        "SELECT relforcerowsecurity FROM pg_class WHERE relname = 'conversations'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(forced.0, "conversations must FORCE ROW LEVEL SECURITY");
}

#[sqlx::test(migrations = "../migrations")]
async fn test_assistant_message_persists_and_reads_back_in_order(pool: sqlx::PgPool) {
    // REC-R1 — a user turn then a final-assistant turn both land in `messages`, read in seq order.
    let repo = MessagesRepo::new(pool.clone());
    let tenant = Uuid::now_v7();
    let user = Uuid::now_v7();
    let session = Uuid::now_v7();
    repo.insert_user(tenant, user, session, "what about embeddings?").await.unwrap();
    repo.insert_assistant(tenant, user, session, "You chose grep recall.").await.unwrap();
    let msgs = repo.read_session(tenant, session).await.unwrap();
    let roles: Vec<&str> = msgs.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, vec!["user", "assistant"], "user then assistant, in seq order");
    assert_eq!(msgs[1].content.as_deref(), Some("You chose grep recall."));
}

#[sqlx::test(migrations = "../migrations")]
async fn test_conversation_upsert_titles_on_first_and_lists_recent(pool: sqlx::PgPool) {
    // REC-R2/REC-D3 — the first upsert sets the title; a later upsert keeps it but is still listed.
    let convo = ConversationsRepo::new(pool.clone());
    let tenant = Uuid::now_v7();
    let user = Uuid::now_v7();
    // the real tenant/user must exist (the conversations FKs require them); seed via MessagesRepo's
    // user path, which idempotently seeds tenants/users (matches the recall caller's real rows).
    let msgs = MessagesRepo::new(pool.clone());
    let thread = Uuid::now_v7();
    msgs.insert_user(tenant, user, thread, "Tell me a very long opening question about Postgres migrations and indexing").await.unwrap();

    convo.upsert(tenant, user, thread, "Tell me a very long opening question about Postgres migrations and indexing").await.unwrap();
    // a second turn keeps the original title (the first user message), only touches updated_at.
    convo.upsert(tenant, user, thread, "and what about partial indexes?").await.unwrap();

    let rows = convo.list(tenant).await.unwrap();
    assert_eq!(rows.len(), 1, "one conversation for the thread");
    assert_eq!(rows[0].id, thread);
    assert!(rows[0].title.starts_with("Tell me a very long opening question"), "title from FIRST user msg");
    assert!(rows[0].title.chars().count() <= 80, "title is truncated: {}", rows[0].title);
}

#[tokio::test]
async fn test_redis_cooldown_cross_worker() {
    if std::env::var("QCUE_TEST_REDIS").is_err() {
        return;
    }
    let cd = Cooldown::connect(&std::env::var("QCUE_TEST_REDIS").unwrap()).await.unwrap();
    let tenant = Uuid::now_v7();
    let cred = Uuid::now_v7();
    cd.set_cooldown(tenant, cred, 60).await.unwrap(); // worker A sets
    let remaining = cd.get_cooldown(tenant, cred).await.unwrap(); // worker B observes
    assert!(remaining.is_some());
}
