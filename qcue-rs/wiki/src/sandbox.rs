//! QCue SBX-R1 — the per-tenant sandbox: the in-process realization of "each user has their own
//! sandbox". It bundles the tenant's writable root (the ONLY writable path; enforced by `path_guard`)
//! and resource quota. RLS (store) and OS hardening (systemd) are the other two pillars (spec §5).
use std::path::{Path, PathBuf};

/// Per-tenant resource caps (disk-fill DoS bound). Cost is bounded separately by the daily ledger.
#[derive(Clone, Copy, Debug)]
pub struct TenantQuota {
    pub max_pages: usize,
    pub max_bytes: u64,
}
impl TenantQuota {
    /// Generous, env-tunable defaults: `QCUE_TENANT_MAX_PAGES` (5000) / `QCUE_TENANT_MAX_VAULT_BYTES` (50 MiB).
    pub fn from_env() -> Self {
        let max_pages = std::env::var("QCUE_TENANT_MAX_PAGES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5_000);
        let max_bytes = std::env::var("QCUE_TENANT_MAX_VAULT_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50 * 1024 * 1024);
        Self { max_pages, max_bytes }
    }
}
impl Default for TenantQuota {
    fn default() -> Self {
        Self { max_pages: 5_000, max_bytes: 50 * 1024 * 1024 }
    }
}

/// A tenant's confinement profile, resolved once per AI task and handed to the write seam.
#[derive(Clone, Debug)]
pub struct TenantSandbox {
    /// The per-tenant vault `t/<tenant>/u/<user>/` — the only writable path.
    pub vault_root: PathBuf,
    pub quota: TenantQuota,
}
impl TenantSandbox {
    /// Build the sandbox for `(tenant, user)` under `data_root` with env-configured quota.
    pub fn for_tenant(data_root: &Path, tenant: uuid::Uuid, user: &str) -> Self {
        Self {
            vault_root: data_root.join(format!("t/{tenant}/u/{user}")),
            quota: TenantQuota::from_env(),
        }
    }
}
