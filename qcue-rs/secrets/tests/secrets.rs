#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R38 — envelope decrypt into a zeroize buffer; central redaction; no plaintext leaks.
use secrets::redact::redact_secrets;
use secrets::{decrypt_with_tenant, EncryptedCredential, EnvKms, Kms, StubKms};

#[test]
fn test_envelope_decrypt_roundtrip() {
    let kms = StubKms::new();
    // encrypt a known key through the stub envelope, then decrypt it back.
    let enc = EncryptedCredential::seal(&kms, b"sk-secret-123", "tenant-A").unwrap();
    let plain = decrypt_with_tenant(&kms, &enc, "tenant-A").unwrap();
    assert_eq!(plain.expose(), b"sk-secret-123");
    // dropping the plaintext zeroizes it (we can't observe the zeroed memory directly,
    // but ZeroizingKey is Drop+Zeroize; assert the type is used).
}

#[test]
fn test_seal_random_uses_fresh_nonce_and_dek_per_call() {
    // The production seal MUST never reuse a (key, nonce) pair — sealing the SAME plaintext twice
    // must produce different nonces AND different ciphertext (no fixed-DEK/fixed-nonce path).
    let kms = StubKms::new();
    let a = EncryptedCredential::seal_random(&kms, b"sk-secret-123", "tenant-A").unwrap();
    let b = EncryptedCredential::seal_random(&kms, b"sk-secret-123", "tenant-A").unwrap();
    assert_ne!(a.key_nonce, b.key_nonce, "nonce must be random per seal");
    assert_ne!(a.key_ciphertext, b.key_ciphertext, "ciphertext must differ per seal");
    assert_ne!(a.dek_wrapped, b.dek_wrapped, "a fresh random DEK must be wrapped per seal");
    // both still decrypt back to the same plaintext.
    assert_eq!(decrypt_with_tenant(&kms, &a, "tenant-A").unwrap().expose(), b"sk-secret-123");
    assert_eq!(decrypt_with_tenant(&kms, &b, "tenant-A").unwrap().expose(), b"sk-secret-123");
}

#[test]
fn test_envkms_roundtrip_and_rejects_wrong_master_key() {
    let kms = EnvKms::from_bytes(&[0x77u8; 48]).expect(">=32 bytes");
    let enc = EncryptedCredential::seal_random(&kms, b"sk-live-XYZ", "tenant-7").unwrap();
    assert_eq!(decrypt_with_tenant(&kms, &enc, "tenant-7").unwrap().expose(), b"sk-live-XYZ");
    // a different master key must NOT unwrap the DEK (AEAD auth failure).
    let other = EnvKms::from_bytes(&[0x99u8; 32]).unwrap();
    assert!(decrypt_with_tenant(&other, &enc, "tenant-7").is_err());
    // and the wrong tenant aad must fail to decrypt the body.
    assert!(decrypt_with_tenant(&kms, &enc, "tenant-OTHER").is_err());
}

#[test]
fn test_envkms_rejects_short_master_key() {
    assert!(EnvKms::from_bytes(&[0u8; 31]).is_none());
    assert!(EnvKms::from_bytes(&[0u8; 32]).is_some());
}

#[test]
fn test_envkms_decrypts_legacy_stub_wrapped_dek() {
    // Backward compatibility: a credential sealed before the crypto hardening (StubKms-wrapped DEK,
    // fixed nonce) must STILL open under EnvKms, so the KMS swap does not strand existing BYOK keys.
    let legacy = EncryptedCredential::seal(&StubKms::new(), b"sk-legacy-001", "tenant-A").unwrap();
    let env = EnvKms::from_bytes(&[0x42u8; 32]).unwrap();
    let plain = decrypt_with_tenant(&env, &legacy, "tenant-A").unwrap();
    assert_eq!(plain.expose(), b"sk-legacy-001");
}

#[test]
fn test_envkms_wrap_is_versioned_and_not_xor() {
    // The real-KMS wrap is tagged with the v1 byte and is NOT the legacy 32-byte XOR blob.
    let env = EnvKms::from_bytes(&[0x11u8; 32]).unwrap();
    let wrapped = env.wrap_dek(&[0xABu8; 32], "tenant-A");
    assert_eq!(wrapped.first(), Some(&0x01u8), "wrapped DEK carries the v1 version byte");
    assert!(wrapped.len() > 32, "real-KMS wrap is longer than the legacy 32-byte XOR blob");
    assert_eq!(env.unwrap_dek(&wrapped, "tenant-A").unwrap(), vec![0xABu8; 32]);
}

#[test]
fn test_redaction_boundary() {
    // S1-R38 / B-R11 — a synthetic message containing a plaintext key is [REDACTED] before write.
    let raw = "my key is sk-ant-api03-ABCDEF0123456789 and openai sk-proj-XYZ987654321";
    let red = redact_secrets(raw);
    assert!(!red.contains("sk-ant-api03-ABCDEF0123456789"));
    assert!(!red.contains("sk-proj-XYZ987654321"));
    assert!(red.contains("[REDACTED]"));
}

#[test]
fn test_redaction_leaves_normal_text() {
    let raw = "the user asked about deployment and databases";
    assert_eq!(redact_secrets(raw), raw);
}
