//! QCue S3-R20 / X2 — the `secrets` façade the vault consumes. The envelope crypto lives in S1's
//! `secrets` crate (`Kms` + AES-256-GCM); this trait wraps it over the Appendix B §4.6 envelope
//! columns. The decrypted plaintext is returned in a `Zeroizing` buffer that zeroes its bytes on Drop
//! (S3-R20) — the secret is NEVER persisted, logged, or returned to the wire (B-R13).
use async_trait::async_trait;
use secrets::{EncryptedCredential, Kms};
use std::sync::Arc;
use uuid::Uuid;

/// The envelope row shape (Appendix B §4.6) the vault persists. No plaintext column (B-R13).
#[derive(Clone, Debug)]
pub struct SealedKey {
    pub key_ciphertext: Vec<u8>,
    pub key_nonce: Vec<u8>,
    pub key_tag: Vec<u8>,
    pub dek_wrapped: Vec<u8>,
    pub kek_id: String,
    pub key_hint: String,
}

/// A buffer that zeroes its bytes on Drop (S3-R20). The harness consumes + drops it; never persisted/logged.
pub struct Zeroizing(Vec<u8>);
impl Zeroizing {
    pub fn new(b: Vec<u8>) -> Self {
        Zeroizing(b)
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("")
    }
}
impl Drop for Zeroizing {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            *b = 0;
        }
    }
}

/// The seam the vault routes consume. `seal` envelope-encrypts a plaintext key; `open` reverses it
/// into a zeroize-on-drop buffer. Async so a real KMS round-trip (network) slots in unchanged.
#[async_trait]
pub trait Secrets: Send + Sync {
    async fn seal(&self, tenant_id: Uuid, plaintext: &str) -> anyhow::Result<SealedKey>;
    async fn open(&self, tenant_id: Uuid, sealed: &SealedKey) -> anyhow::Result<Zeroizing>;
}

/// Compute the UI / `api_key_hint` (pitfall #4) — last 3 chars, never the key.
pub fn key_hint(plaintext: &str) -> String {
    let chars: Vec<char> = plaintext.chars().collect();
    let tail: String = chars.iter().rev().take(3).rev().collect();
    format!("…{tail}")
}

/// The default `Secrets` impl, backed by the S1 `secrets` crate's `Kms` + AES-256-GCM envelope.
/// The GCM auth tag (last 16 bytes of the ciphertext) is split into the `key_tag` column so the
/// Appendix B §4.6 envelope shape (separate `key_tag BYTEA NOT NULL`) is satisfied.
pub struct KmsSecrets {
    kms: Arc<dyn Kms + Send + Sync>,
    kek_id: String,
}
impl KmsSecrets {
    pub fn new(kms: Arc<dyn Kms + Send + Sync>, kek_id: impl Into<String>) -> Self {
        Self { kms, kek_id: kek_id.into() }
    }
}

const GCM_TAG_LEN: usize = 16;

#[async_trait]
impl Secrets for KmsSecrets {
    async fn seal(&self, tenant_id: Uuid, plaintext: &str) -> anyhow::Result<SealedKey> {
        let tenant = tenant_id.to_string();
        // Production seal: a fresh random DEK + fresh random nonce per credential (NEVER the fixed-DEK/
        // fixed-nonce `seal` test helper — reusing a nonce in AES-GCM is catastrophic).
        let enc = EncryptedCredential::seal_random(self.kms.as_ref(), plaintext.as_bytes(), &tenant)
            .map_err(|e| anyhow::anyhow!("seal failed: {e}"))?;
        // Split the appended GCM tag into its own column (Appendix B §4.6 separate key_tag).
        let mut ct = enc.key_ciphertext;
        let tag = if ct.len() >= GCM_TAG_LEN { ct.split_off(ct.len() - GCM_TAG_LEN) } else { Vec::new() };
        Ok(SealedKey {
            key_ciphertext: ct,
            key_nonce: enc.key_nonce,
            key_tag: tag,
            dek_wrapped: enc.dek_wrapped,
            kek_id: self.kek_id.clone(),
            key_hint: key_hint(plaintext),
        })
    }

    async fn open(&self, tenant_id: Uuid, sealed: &SealedKey) -> anyhow::Result<Zeroizing> {
        let tenant = tenant_id.to_string();
        // re-join ciphertext + tag for the AEAD open.
        let mut ct = sealed.key_ciphertext.clone();
        ct.extend_from_slice(&sealed.key_tag);
        let enc = EncryptedCredential {
            key_ciphertext: ct,
            key_nonce: sealed.key_nonce.clone(),
            dek_wrapped: sealed.dek_wrapped.clone(),
        };
        let z = secrets::decrypt_with_tenant(self.kms.as_ref(), &enc, &tenant)
            .map_err(|e| anyhow::anyhow!("open failed: {e}"))?;
        Ok(Zeroizing::new(z.expose().to_vec()))
    }
}
