//! QCue S3 — the BYOK keys vault: the `secrets` seal/open façade (secrets) + the management API
//! (routes). The plaintext key never leaves the seal boundary; reads echo only the `key_hint`/status.
pub mod routes;
pub mod secrets;
