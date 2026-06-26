// QCue S1-R79 — STT result envelope (never raised; success flag carries failure).
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TranscriptionResult {
    pub success: bool,
    pub transcript: String,
    pub error: Option<String>,
    pub provider: String,
}
