// QCue S1-R56 — messages superset persistence (Appendix B §4.13). user persisted before the call.
use sqlx::PgPool;
use uuid::Uuid;

pub struct MsgRow {
    pub content: Option<String>,
    pub role: String,
}

pub struct MessagesRepo {
    pool: PgPool,
}
impl MessagesRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Kept for plan-API compatibility; the RLS GUC is applied per-transaction inside each method
    /// (a pooled `set_config(...,false)` only affects one connection and would not carry across the
    /// pool), so this is a no-op acknowledgement of the request tenant.
    pub async fn set_tenant_guc(&self, _tenant: Uuid) -> Result<(), sqlx::Error> {
        Ok(())
    }

    /// Insert a user-role message (persisted before the provider call; S1-R56).
    ///
    /// NOTE (deviation from plan): `messages.tenant_id`/`messages.user_id` have hard FKs to
    /// `tenants`/`users` in the Appendix B schema, so the parent rows are seeded idempotently first
    /// (the plan's INSERT alone would violate those FKs against the verbatim DDL). `users.email` is
    /// globally UNIQUE, so the seed derives a per-user email and skips on conflict. The whole
    /// operation runs in one transaction with `SET LOCAL app.tenant_id` so RLS (B-R4/B-R5) passes.
    pub async fn insert_user(
        &self,
        tenant: Uuid,
        user: Uuid,
        session: Uuid,
        content: &str,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO tenants (id, slug, display_name, namespace)
             VALUES ($1, $2, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(tenant)
        .bind(tenant.to_string())
        .bind(format!("t/{tenant}"))
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO users (id, tenant_id, email)
             VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user)
        .bind(tenant)
        .bind(format!("{user}@seed.qcue"))
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO messages (tenant_id, session_id, user_id, role, content) VALUES ($1,$2,$3,'user',$4)")
            .bind(tenant)
            .bind(session)
            .bind(user)
            .bind(content)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Insert a final-assistant message (REC-R1/REC-D6). Persists ONLY the final assistant TEXT —
    /// no raw tool steps, no `tool_calls`/`provider_data` (those are redacted at the persistence
    /// boundary, S1-R38). Unlike `insert_user` this does NOT seed `tenants`/`users`: the recall caller
    /// already resolved real `tenant_id`/`user_id` from the JWT (`TenantCtx`), and those rows exist.
    pub async fn insert_assistant(
        &self,
        tenant: Uuid,
        user: Uuid,
        session: Uuid,
        content: &str,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO messages (tenant_id, session_id, user_id, role, content) VALUES ($1,$2,$3,'assistant',$4)")
            .bind(tenant)
            .bind(session)
            .bind(user)
            .bind(content)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn read_session(&self, tenant: Uuid, session: Uuid) -> Result<Vec<MsgRow>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        // Bound the history we load into a recall request: keep the most-recent 1000 active messages
        // (in chronological order). A normal thread is far smaller; this only caps a pathologically long
        // session so one thread can't drive an unbounded query/allocation.
        let rows = sqlx::query_as::<_, (Option<String>, String)>(
            "SELECT content, role FROM ( \
               SELECT content, role::text AS role, seq FROM messages \
               WHERE tenant_id=$1 AND session_id=$2 AND active ORDER BY seq DESC LIMIT 1000 \
             ) recent ORDER BY seq",
        )
        .bind(tenant)
        .bind(session)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(|(content, role)| MsgRow { content, role }).collect())
    }
}

/// A conversation header row for the recall history drawer (REC-R3). `last_snippet` is the most recent
/// message body (optional; the drawer shows it under the title).
pub struct ConvoRow {
    pub id: Uuid,
    pub title: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub last_snippet: Option<String>,
}

/// The `conversations` header repo (REC-D2): one row per recall thread. `upsert` is called once per
/// turn in the same logical op as the message writes; `list` backs `GET /v1/conversations`.
pub struct ConversationsRepo {
    pool: PgPool,
}
impl ConversationsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Upsert the conversation header for `thread`. The title is set from `first_or_current` ONLY on
    /// the first insert (REC-D3); a later turn keeps the original title and just touches `updated_at`
    /// (the `conversations_touch` trigger). `tenant`/`user` are JWT-resolved real rows (RLS).
    pub async fn upsert(
        &self,
        tenant: Uuid,
        user: Uuid,
        thread: Uuid,
        first_or_current: &str,
    ) -> Result<(), sqlx::Error> {
        let title = derive_title(first_or_current);
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        // ON CONFLICT: keep the existing title (first message wins); a no-op UPDATE still fires the
        // BEFORE UPDATE trigger so updated_at advances → the row re-sorts to the top of the drawer.
        sqlx::query(
            "INSERT INTO conversations (id, tenant_id, user_id, title) VALUES ($1,$2,$3,$4) \
             ON CONFLICT (id) DO UPDATE SET title = conversations.title",
        )
        .bind(thread)
        .bind(tenant)
        .bind(user)
        .bind(&title)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// List the tenant's conversations newest-first with the latest message snippet (REC-R3). Joins the
    /// most-recent active `messages` body per thread for the drawer subtitle.
    pub async fn list(&self, tenant: Uuid) -> Result<Vec<ConvoRow>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        let rows = sqlx::query_as::<_, (Uuid, String, chrono::DateTime<chrono::Utc>, Option<String>)>(
            "SELECT c.id, c.title, c.updated_at, \
                    (SELECT m.content FROM messages m \
                       WHERE m.tenant_id = c.tenant_id AND m.session_id = c.id AND m.active \
                       ORDER BY m.seq DESC LIMIT 1) AS last_snippet \
             FROM conversations c WHERE c.tenant_id = $1 ORDER BY c.updated_at DESC LIMIT 100",
        )
        .bind(tenant)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|(id, title, updated_at, last_snippet)| ConvoRow { id, title, updated_at, last_snippet })
            .collect())
    }
}

/// Derive a conversation title from the first user message (REC-D3): collapse whitespace, truncate to
/// 80 chars on a char boundary, fall back to a default for an empty question. No LLM call.
fn derive_title(s: &str) -> String {
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return "New conversation".to_string();
    }
    collapsed.chars().take(80).collect()
}
