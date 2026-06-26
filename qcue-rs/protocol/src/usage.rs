// QCue S1-R66 — CanonicalUsage keeps its 5th `reasoning` field (a real ledger token class).
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CanonicalUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}
