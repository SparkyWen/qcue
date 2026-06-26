// QCue S3-R20..R23 — the credential resolver seam. The router stays DB-free: it only declares the
// trait the real HttpDispatch calls; the DB+vault impl lives in app-server (`DbVaultResolver`).
use crate::pool::CredentialPool;
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug)]
pub enum ResolveError {
    NotFound,
    Decrypt(String),
    Db(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound => write!(f, "credential not found"),
            ResolveError::Decrypt(e) => write!(f, "decrypt failed: {e}"),
            ResolveError::Db(e) => write!(f, "db error: {e}"),
        }
    }
}
impl std::error::Error for ResolveError {}

#[async_trait]
pub trait CredentialResolver: Send + Sync {
    /// Build the per-(tenant, provider) credential pool from persisted rows (S3-R20).
    async fn pool_for(&self, tenant: Uuid, provider: &str) -> Result<CredentialPool, ResolveError>;
    /// Decrypt the credential's plaintext API key into a zeroize-on-drop buffer (S1-R38).
    async fn decrypt(
        &self,
        tenant: Uuid,
        cred_id: Uuid,
    ) -> Result<secrets::ZeroizingKey, ResolveError>;
    /// Best-effort write-back of status/cooldown/dead after the call (S3-R23). MVP: may no-op.
    async fn persist_transitions(
        &self,
        _tenant: Uuid,
        _provider: &str,
        _pool: &CredentialPool,
    ) -> Result<(), ResolveError> {
        Ok(())
    }
}
