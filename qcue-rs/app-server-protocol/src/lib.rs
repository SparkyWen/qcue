//! QCue app-server-protocol — data-only versioned wire schemas (Master §8). pitfall #17: no logic here.
//! `RuntimeEventEnvelope`/`WikiEditOp`/`DreamPhase` are the canonical single-definition `protocol`-crate types
//! (re-exported through `envelope`/`items` below) — NOT redefined here.
pub mod envelope;
pub mod items;
pub mod v1;
pub mod v2;

pub use envelope::{error_codes, Message, RpcError, RuntimeEvent, RuntimeEventEnvelope};
pub use items::{Citation, DreamPhase, Item, Role, WikiEditOp};
