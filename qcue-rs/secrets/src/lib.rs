// QCue S1-R38 — envelope decryption + zeroize-on-drop plaintext. Never persists/logs the key.
pub mod redact;

use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use thiserror::Error;
use zeroize::Zeroize;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("kms unwrap failed")]
    KmsUnwrap,
    #[error("aead decrypt failed")]
    Decrypt,
    #[error("aead encrypt failed")]
    Encrypt,
}

/// A KMS that wraps/unwraps the per-tenant DEK. StubKms is the keyless test impl.
pub trait Kms {
    fn wrap_dek(&self, dek: &[u8], tenant: &str) -> Vec<u8>;
    fn unwrap_dek(&self, wrapped: &[u8], tenant: &str) -> Result<Vec<u8>, SecretError>;
}

/// Deterministic XOR-based "KMS" for tests only (NOT for production; S3 supplies a real KMS).
pub struct StubKms {
    kek: [u8; 32],
}
impl StubKms {
    pub fn new() -> Self {
        Self { kek: [0x5Au8; 32] }
    }
}
impl Default for StubKms {
    fn default() -> Self {
        Self::new()
    }
}
impl Kms for StubKms {
    fn wrap_dek(&self, dek: &[u8], _tenant: &str) -> Vec<u8> {
        dek.iter().enumerate().map(|(i, b)| b ^ self.kek[i % 32]).collect()
    }
    fn unwrap_dek(&self, wrapped: &[u8], _tenant: &str) -> Result<Vec<u8>, SecretError> {
        Ok(wrapped.iter().enumerate().map(|(i, b)| b ^ self.kek[i % 32]).collect())
    }
}

/// Version byte for an [`EnvKms`]-wrapped DEK. The legacy [`StubKms`] format is a bare XOR blob the same
/// length as the DEK (32 bytes) whose first byte is the constant `0x11 ^ 0x5A = 0x4B`, so a leading
/// `0x01` unambiguously marks the real-KMS format and lets one `unwrap_dek` decode both.
const ENV_WRAP_V1: u8 = 0x01;
/// The (public, constant) [`StubKms`] KEK. Embedded here ONLY so [`EnvKms`] can still decrypt DEKs that
/// were XOR-wrapped before the crypto hardening — keeping pre-upgrade BYOK keys readable until a vault
/// re-write re-seals them under the real master key. These records were never truly confidential.
const LEGACY_STUB_KEK: [u8; 32] = [0x5Au8; 32];

/// The production KMS: wraps each per-credential DEK with AES-256-GCM under a server-held master key
/// (`QCUE_KMS_KEY`, the only long-lived secret). This replaces [`StubKms`] (whose KEK is a hardcoded
/// public constant — useless for real confidentiality). `unwrap_dek` transparently also decodes legacy
/// stub-wrapped DEKs, so swapping the KMS in does not strand keys sealed before the upgrade.
pub struct EnvKms {
    kek: [u8; 32],
}
impl EnvKms {
    /// Build from raw master-key bytes; the first 32 bytes become the AES-256 KEK. Returns `None` for a
    /// master key shorter than 32 bytes (refuse a weak key, mirroring the JWT-secret floor).
    pub fn from_bytes(master: &[u8]) -> Option<Self> {
        if master.len() < 32 {
            return None;
        }
        let mut kek = [0u8; 32];
        kek.copy_from_slice(&master[..32]);
        Some(Self { kek })
    }
}
impl Drop for EnvKms {
    fn drop(&mut self) {
        self.kek.zeroize();
    }
}
impl Kms for EnvKms {
    fn wrap_dek(&self, dek: &[u8], tenant: &str) -> Vec<u8> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.kek));
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        // Encrypt is infallible for these sizes; on the impossible error path emit an empty blob so
        // `unwrap_dek` fails closed rather than ever returning a usable DEK (and avoids the unwrap lint).
        let ct = cipher
            .encrypt(&nonce, Payload { msg: dek, aad: tenant.as_bytes() })
            .unwrap_or_default();
        let mut out = Vec::with_capacity(1 + nonce.len() + ct.len());
        out.push(ENV_WRAP_V1);
        out.extend_from_slice(nonce.as_slice());
        out.extend_from_slice(&ct);
        out
    }
    fn unwrap_dek(&self, wrapped: &[u8], tenant: &str) -> Result<Vec<u8>, SecretError> {
        match wrapped.split_first() {
            // Real-KMS format: 0x01 || nonce(12) || ciphertext+tag(>=16).
            Some((&ENV_WRAP_V1, rest)) if rest.len() >= 12 + 16 => {
                let (nonce_bytes, ct) = rest.split_at(12);
                let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.kek));
                cipher
                    .decrypt(Nonce::from_slice(nonce_bytes), Payload { msg: ct, aad: tenant.as_bytes() })
                    .map_err(|_| SecretError::KmsUnwrap)
            }
            // Legacy XOR-stub-wrapped DEK (sealed before the crypto hardening) — kept decryptable so a
            // later vault PUT transparently re-seals it under the real master key.
            _ => Ok(wrapped.iter().enumerate().map(|(i, b)| b ^ LEGACY_STUB_KEK[i % 32]).collect()),
        }
    }
}

/// The persisted ciphertext columns (mirror provider_credentials, Appendix B §4.6).
pub struct EncryptedCredential {
    pub key_ciphertext: Vec<u8>,
    pub key_nonce: Vec<u8>,
    pub dek_wrapped: Vec<u8>,
}

/// A plaintext key buffer that zeroizes on drop (never persisted, never logged).
pub struct ZeroizingKey(Vec<u8>);
impl ZeroizingKey {
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}
impl Drop for ZeroizingKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl EncryptedCredential {
    /// Test/seed helper: seal a plaintext key with a fresh DEK wrapped by the KMS.
    pub fn seal(kms: &dyn Kms, plaintext: &[u8], tenant: &str) -> Result<Self, SecretError> {
        let dek = [0x11u8; 32]; // fixed test DEK; production generates a random DEK
        let cipher = Aes256Gcm::new_from_slice(&dek).map_err(|_| SecretError::Encrypt)?;
        let nonce_bytes = [0x22u8; 12];
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(nonce, Payload { msg: plaintext, aad: tenant.as_bytes() })
            .map_err(|_| SecretError::Encrypt)?;
        Ok(Self {
            key_ciphertext: ct,
            key_nonce: nonce_bytes.to_vec(),
            dek_wrapped: kms.wrap_dek(&dek, tenant),
        })
    }

    /// Production seal (S1-R38): a FRESH cryptographically-random 256-bit DEK + a FRESH random 96-bit
    /// nonce for EVERY credential, so no two stored secrets ever share a (key, nonce) pair. The DEK is
    /// wrapped by the KMS (whose KEK is the only long-lived secret) and zeroized immediately after; only
    /// the wrapped DEK + nonce + ciphertext are persisted. Unlike [`EncryptedCredential::seal`] — a
    /// fixed-DEK/fixed-nonce helper that exists ONLY for deterministic test/seed fixtures — this is the
    /// only sealing path safe for real keys (a fixed nonce in AES-GCM is catastrophic).
    pub fn seal_random(kms: &dyn Kms, plaintext: &[u8], tenant: &str) -> Result<Self, SecretError> {
        let mut dek = [0u8; 32];
        OsRng.fill_bytes(&mut dek);
        let cipher = Aes256Gcm::new_from_slice(&dek).map_err(|_| SecretError::Encrypt)?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ct = cipher
            .encrypt(&nonce, Payload { msg: plaintext, aad: tenant.as_bytes() })
            .map_err(|_| SecretError::Encrypt)?;
        let dek_wrapped = kms.wrap_dek(&dek, tenant);
        dek.zeroize();
        Ok(Self { key_ciphertext: ct, key_nonce: nonce.as_slice().to_vec(), dek_wrapped })
    }
}

/// S1-R38 — unwrap DEK via KMS → AES-GCM decrypt into a zeroize buffer.
///
/// Back-compat shim: the original signature hardcodes the `"tenant-A"` aad/KMS-context. New callers
/// (S3 vault, per-tenant) use [`decrypt_with_tenant`] so the round-trip is correct for any tenant.
pub fn decrypt(kms: &dyn Kms, enc: &EncryptedCredential) -> Result<ZeroizingKey, SecretError> {
    decrypt_with_tenant(kms, enc, "tenant-A")
}

/// S1-R38 (per-tenant) — unwrap DEK via KMS for `tenant` → AES-GCM decrypt with `tenant` as the AEAD
/// aad, into a zeroize buffer. `tenant` MUST match the value passed to [`EncryptedCredential::seal`].
pub fn decrypt_with_tenant(
    kms: &dyn Kms,
    enc: &EncryptedCredential,
    tenant: &str,
) -> Result<ZeroizingKey, SecretError> {
    let dek = kms.unwrap_dek(&enc.dek_wrapped, tenant)?;
    let cipher = Aes256Gcm::new_from_slice(&dek).map_err(|_| SecretError::Decrypt)?;
    let nonce = Nonce::from_slice(&enc.key_nonce);
    let pt = cipher
        .decrypt(nonce, Payload { msg: &enc.key_ciphertext, aad: tenant.as_bytes() })
        .map_err(|_| SecretError::Decrypt)?;
    Ok(ZeroizingKey(pt))
}
