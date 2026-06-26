//! QCue v2 — coexists with v1 (S3-R43). ThreadStart gains `labels` without breaking v1 clients.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct ThreadStartParams {
    pub kind: String,
    #[serde(default)]
    pub labels: Vec<String>,
}
