//! QCue S3-R20..R23 — `DbVaultResolver`: the app-server impl of `router::resolver::CredentialResolver`.
//!
//! The router stays DB-free; the live `HttpDispatch` reaches credentials ONLY through this seam:
//!   - `pool_for`  loads the `provider_credentials` rows for (tenant, provider) into a
//!     `router::pool::CredentialPool` (the 3-state machine: ok | exhausted | dead).
//!   - `decrypt`   reads the row's envelope columns into a `vault::secrets::SealedKey`, calls
//!     `Secrets::open` (KMS unwrap → AES-256-GCM), and re-wraps the plaintext in the router's
//!     `secrets::ZeroizingKey` (zeroize-on-drop; the secret is never logged/persisted/returned).
//!   - `persist_transitions` writes status/cooldown_until/dead_at back (best-effort; a minimal UPDATE).
//!
//! Every query binds `app.tenant_id` in its own tx (the `provider_credentials` table is FORCE RLS, so
//! a pooled connection MUST re-apply the GUC per tx — matching `store::creds_repo`).
use async_trait::async_trait;
use router::pool::{CredentialPool, PoolStrategy, PooledCredential};
use router::resolver::{CredentialResolver, ResolveError};
use secrets::{EncryptedCredential, Kms};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

const GCM_TAG_LEN: usize = 16;

/// The DB+vault credential resolver. Holds the `qcue_app` pool (RLS-bound per tx) and the KMS that
/// unwraps the per-tenant DEK (the plaintext key is decrypted into a zeroize-on-drop buffer and never
/// persisted/logged/returned to the wire — B-R13). The KMS is the same one `vault::secrets::KmsSecrets`
/// wraps; the resolver decrypts directly so it can return the router-facing `secrets::ZeroizingKey`.
pub struct DbVaultResolver {
    pool: PgPool,
    kms: Arc<dyn Kms + Send + Sync>,
}

impl DbVaultResolver {
    pub fn new(pool: PgPool, kms: Arc<dyn Kms + Send + Sync>) -> Self {
        Self { pool, kms }
    }

    /// Map the persisted `cred_status` string to the router's `protocol::CredStatus`.
    fn status_from_db(status: &str, cooldown_until_ms: Option<i64>) -> protocol::CredStatus {
        match status {
            "dead" => protocol::CredStatus::Dead,
            "exhausted" => protocol::CredStatus::Exhausted {
                until_ms: cooldown_until_ms.unwrap_or(0),
            },
            _ => protocol::CredStatus::Ok,
        }
    }
}

#[async_trait]
impl CredentialResolver for DbVaultResolver {
    /// S3-R20 — build the per-(tenant, provider) pool from the persisted rows. RLS GUC bound in-tx.
    async fn pool_for(
        &self,
        tenant: Uuid,
        provider: &str,
    ) -> Result<CredentialPool, ResolveError> {
        let mut tx = self.pool.begin().await.map_err(|e| ResolveError::Db(e.to_string()))?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| ResolveError::Db(e.to_string()))?;
        // `cooldown_until` is read as epoch-ms so the pool's `now_ms`-based eligibility check works.
        let rows = sqlx::query(
            "SELECT id, label, priority, status::text AS status, key_hint, last_error_code,
                    last_error_reason, request_count,
                    (EXTRACT(EPOCH FROM cooldown_until) * 1000)::BIGINT AS cooldown_ms
             FROM provider_credentials
             WHERE tenant_id = $1 AND provider = $2",
        )
        .bind(tenant)
        .bind(provider)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| ResolveError::Db(e.to_string()))?;
        tx.commit().await.map_err(|e| ResolveError::Db(e.to_string()))?;

        if rows.is_empty() {
            return Err(ResolveError::NotFound);
        }

        let entries: Vec<PooledCredential> = rows
            .iter()
            .map(|r| {
                let status: String = r.get("status");
                let cooldown_ms: Option<i64> = r.try_get("cooldown_ms").ok().flatten();
                PooledCredential {
                    id: r.get("id"),
                    label: r.try_get::<Option<String>, _>("label").ok().flatten(),
                    priority: r.get::<i32, _>("priority"),
                    status: Self::status_from_db(&status, cooldown_ms),
                    key_hint: r.get("key_hint"),
                    last_error_code: r
                        .try_get::<Option<i32>, _>("last_error_code")
                        .ok()
                        .flatten()
                        .map(|c| c as u16),
                    last_error_reason: r
                        .try_get::<Option<String>, _>("last_error_reason")
                        .ok()
                        .flatten(),
                    request_count: r.get::<i64, _>("request_count").max(0) as u64,
                }
            })
            .collect();
        // FillFirst honors the `priority` column (the pool selection hot path; pitfall #3).
        Ok(CredentialPool::new(entries, PoolStrategy::FillFirst))
    }

    /// S1-R38 — read the envelope columns for `cred_id` and KMS-unwrap → AES-GCM decrypt into a
    /// zeroize-on-drop buffer. The plaintext NEVER touches a log/column/wire (B-R13).
    async fn decrypt(
        &self,
        tenant: Uuid,
        cred_id: Uuid,
    ) -> Result<secrets::ZeroizingKey, ResolveError> {
        let mut tx = self.pool.begin().await.map_err(|e| ResolveError::Db(e.to_string()))?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| ResolveError::Db(e.to_string()))?;
        let row = sqlx::query(
            "SELECT key_ciphertext, key_nonce, key_tag, dek_wrapped
             FROM provider_credentials WHERE id = $1 AND tenant_id = $2",
        )
        .bind(cred_id)
        .bind(tenant)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| ResolveError::Db(e.to_string()))?;
        tx.commit().await.map_err(|e| ResolveError::Db(e.to_string()))?;

        let row = row.ok_or(ResolveError::NotFound)?;
        // Re-join the GCM auth tag (split into its own column per Appendix B §4.6) onto the ciphertext
        // for the AEAD open — mirrors `vault::secrets::KmsSecrets::open`. An all-zero placeholder row
        // (the test seed) decrypts to garbage/errors, which the dispatch loop treats as a decrypt miss.
        let mut ct: Vec<u8> = row.get("key_ciphertext");
        let tag: Vec<u8> = row.get("key_tag");
        if !tag.is_empty() || ct.len() < GCM_TAG_LEN {
            ct.extend_from_slice(&tag);
        }
        let enc = EncryptedCredential {
            key_ciphertext: ct,
            key_nonce: row.get("key_nonce"),
            dek_wrapped: row.get("dek_wrapped"),
        };
        // KMS unwrap → AES-256-GCM decrypt into a zeroize-on-drop buffer (the router-facing type the
        // resolver trait declares). The `tenant` string is the AEAD aad (must match `seal`).
        secrets::decrypt_with_tenant(self.kms.as_ref(), &enc, &tenant.to_string())
            .map_err(|e| ResolveError::Decrypt(e.to_string()))
    }

    /// S3-R23 — best-effort write-back of the post-call pool transitions (status/cooldown/dead). The
    /// pool exposes the live state via `find`; we mirror exhausted/dead for each known key_hint.
    async fn persist_transitions(
        &self,
        tenant: Uuid,
        provider: &str,
        pool: &CredentialPool,
    ) -> Result<(), ResolveError> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut tx = self.pool.begin().await.map_err(|e| ResolveError::Db(e.to_string()))?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| ResolveError::Db(e.to_string()))?;
        // Re-read the (tenant, provider) ids so we only UPDATE rows we own, then mirror the in-memory
        // pool's status onto each. A minimal best-effort write (the live timer lives in Redis).
        let ids = sqlx::query(
            "SELECT id, key_hint FROM provider_credentials WHERE tenant_id = $1 AND provider = $2",
        )
        .bind(tenant)
        .bind(provider)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| ResolveError::Db(e.to_string()))?;
        for r in &ids {
            let id: Uuid = r.get("id");
            let hint: String = r.get("key_hint");
            let Some(c) = pool.find(&hint) else { continue };
            match c.status {
                protocol::CredStatus::Dead => {
                    let _ = sqlx::query(
                        "UPDATE provider_credentials SET status='dead', dead_at = now()
                         WHERE id = $1 AND status <> 'dead'",
                    )
                    .bind(id)
                    .execute(&mut *tx)
                    .await;
                }
                protocol::CredStatus::Exhausted { until_ms } => {
                    // until_ms is epoch-ms; convert to an interval from now for the cooldown column.
                    let secs = ((until_ms - now_ms).max(0)) as f64 / 1000.0;
                    let _ = sqlx::query(
                        "UPDATE provider_credentials
                         SET status='exhausted', cooldown_until = now() + make_interval(secs => $2)
                         WHERE id = $1",
                    )
                    .bind(id)
                    .bind(secs)
                    .execute(&mut *tx)
                    .await;
                }
                protocol::CredStatus::Ok => {
                    // S1-R35 — a credential that just succeeded heals: clear any cooldown so the next
                    // ingest/recall/dream run can use it immediately (it was previously stuck
                    // `exhausted` until the user re-saved the key in Settings — the "stuck for 8 hours"
                    // report). Scoped to a still-`exhausted` row so a healthy cred is never churned.
                    let _ = sqlx::query(
                        "UPDATE provider_credentials \
                         SET status='ok', cooldown_until=NULL, last_error_code=NULL \
                         WHERE id = $1 AND status = 'exhausted'",
                    )
                    .bind(id)
                    .execute(&mut *tx)
                    .await;
                }
            }
        }
        tx.commit().await.map_err(|e| ResolveError::Db(e.to_string()))?;
        Ok(())
    }
}
