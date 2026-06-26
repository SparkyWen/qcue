//! QCue S3-R8/R10 — argon2id {m=19456,t=2,p=1}, DUMMY_HASH constant-time path, lazy rehash.
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};

// The policy params (Clariose: m=19456, t=2, p=1).
fn argon() -> Result<Argon2<'static>, argon2::password_hash::Error> {
    let params = Params::new(19456, 2, 1, None)
        .map_err(|_| argon2::password_hash::Error::ParamValueInvalid(argon2::password_hash::errors::InvalidValue::InvalidFormat))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

// A fixed valid argon2id hash so unknown-email verifies in constant time (never early-return).
// Generated once via `hash_password("qcue-dummy-constant-time")` with the policy params above (S3-R8).
pub const DUMMY_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$ALf/FWaOcGnBQa5DHH/hzA$hFKcfzBUOZyT3Zlfu6WtMD7uRnPC3M7t0PQA64J7Zpk";

pub fn hash_password(pw: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(argon()?.hash_password(pw.as_bytes(), &salt)?.to_string())
}

#[derive(Debug, PartialEq)]
pub enum VerifyOutcome {
    Match,
    Mismatch,
}

/// Verify; on `None` (unknown email) verify against DUMMY_HASH anyway — constant-time (S3-R8).
pub fn verify_password(stored: Option<&str>, candidate: &str) -> VerifyOutcome {
    let hash_str = stored.unwrap_or(DUMMY_HASH);
    let parsed = match PasswordHash::new(hash_str) {
        Ok(p) => p,
        Err(_) => return VerifyOutcome::Mismatch,
    };
    let argon = match argon() {
        Ok(a) => a,
        Err(_) => return VerifyOutcome::Mismatch,
    };
    match argon.verify_password(candidate.as_bytes(), &parsed) {
        Ok(()) if stored.is_some() => VerifyOutcome::Match,
        _ => VerifyOutcome::Mismatch,
    }
}

/// True when a stored hash's params differ from current policy (S3-R10).
pub fn needs_rehash(stored: &str) -> bool {
    match PasswordHash::new(stored) {
        Ok(p) => {
            let m = p.params.get("m").and_then(|v| v.decimal().ok()).unwrap_or(0);
            let t = p.params.get("t").and_then(|v| v.decimal().ok()).unwrap_or(0);
            let par = p.params.get("p").and_then(|v| v.decimal().ok()).unwrap_or(0);
            !(m == 19456 && t == 2 && par == 1)
        }
        Err(_) => true,
    }
}
