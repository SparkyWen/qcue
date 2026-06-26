// QCue S1-R30 — 3-state credential status; cooldown lives in the type.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `until_ms` is a millisecond epoch deadline (serde-portable; the pool compares to `now_ms()`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum CredStatus {
    Ok,
    Exhausted { until_ms: i64 },
    Dead,
}
