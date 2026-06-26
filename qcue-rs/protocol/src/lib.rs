//! QCue protocol crate — serde-only shared types. No async, no reqwest, no sqlx, no tokio.
#![allow(clippy::module_inception)]
pub mod api_mode;
pub mod appendix_a;
pub mod cred;
pub mod envelope;
pub mod error;
pub mod ids;
pub mod message;
pub mod observe;
pub mod response;
pub mod stream;
pub mod sync;
pub mod transcription;
pub mod usage;

pub use api_mode::ApiMode;
pub use appendix_a::{Citation, ConversationMessage, ConversationSummary, DreamPhase, WikiEditOp};
pub use cred::CredStatus;
pub use envelope::RuntimeEventEnvelope;
pub use error::{ApiError, ClassifiedError, FailoverReason, TransportError};
pub use ids::{FirstClassProvider, ProviderId};
pub use message::{Message, Role, ToolCall, ToolDef};
pub use observe::TurnEventSink;
pub use response::{FinishReason, NormalizedResponse};
pub use stream::{Block, Delta, StreamEvent, StreamEventBox};
pub use sync::{IdeaSnap, SyncDelta, SyncOp, SyncSnapshot, WikiPageSnap};
pub use transcription::TranscriptionResult;
pub use usage::CanonicalUsage;
