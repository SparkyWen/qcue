//! QCue v1 wire method params/results (S3-R43). All inbound DTOs deny unknown fields (B-R8).
use crate::items::Item;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct InitializeParams {
    #[serde(default)]
    pub opt_out_notification_methods: Vec<String>,
    #[serde(default)]
    pub schema_version: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct ThreadStartParams {
    pub kind: String,
} // "recall"|"wiki"|"idea"|"dream"

#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct TurnStartParams {
    pub thread_id: Uuid,
    pub input: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct CaptureParams {
    pub kind: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    pub origin: String,
    /// LOC-R3/Part F — the client's action-time capture instant (RFC3339 UTC). None ⇒ server now().
    #[serde(default)]
    pub captured_at: Option<String>,
    /// LOC-R1 — optional precise location captured at action-time (off by default).
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lng: Option<f64>,
    #[serde(default)]
    pub loc_accuracy_m: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
pub struct CaptureResult {
    pub idea_id: Uuid,
    pub ingest_job_id: Uuid,
}

/// The full detail of one capture (GET /v1/captures/{id}) — CAP-R1.
#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
pub struct CaptureDetail {
    pub id: Uuid,
    pub kind: String,
    pub body: String,
    pub captured_at: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub loc_accuracy_m: Option<f32>,
    pub ingest_state: String,
    pub source_url: Option<String>,
    pub origin: String,
    /// The slug of the distilled SOURCE page (DIG-R4), if this capture has been ingested. None ⇒ pending.
    pub source_page_slug: Option<String>,
}

/// Edit one capture (PATCH /v1/captures/{id}) — CAP-R2. Absent field ⇒ leave unchanged.
#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct CapturePatch {
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lng: Option<f64>,
    #[serde(default)]
    pub loc_accuracy_m: Option<f32>,
}

/// DIG-R5 — the result of `POST /v1/ingest/run` (the one-click incremental digest): how many per-idea
/// ingest jobs were enqueued, and their ids (so the client can poll `GET /v1/jobs/{id}` for progress).
/// Repeated clicks collapse onto existing pending jobs (debounce), so `enqueued` counts distinct dirty
/// ideas, not new rows.
#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
pub struct IngestRunResult {
    pub enqueued: u32,
    pub job_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
pub struct TurnResult {
    pub turn_id: Uuid,
    pub items: Vec<Item>,
}

/// AU-R7 — the app release manifest (`GET /v1/app/release?platform=…`). Global, non-tenant, no secrets.
/// `latest_build`/`min_supported_build` are the integer `pubspec.yaml` build numbers the client compares
/// against its own. `android_apk_path` (Android only) points at the JWT-authenticated proxy
/// `/v1/app/apk/{build}`; `ios_app_store_url` (iOS only) is the App Store deep link.
#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[ts(export)]
pub struct AppReleaseManifest {
    pub platform: String,
    pub latest_build: u32,
    pub latest_version: String,
    pub min_supported_build: u32,
    pub changelog: String,
    pub android_apk_path: Option<String>,
    /// Android only — the expected SHA-256 (lowercase hex) of the APK served at `android_apk_path`, so
    /// the client can verify the sideloaded download's integrity (tamper/MITM detection). None ⇒ the
    /// server has no hash on file (older manifest) and the client treats it as unverified.
    #[serde(default)]
    pub android_apk_sha256: Option<String>,
    pub ios_app_store_url: Option<String>,
    pub published_at: String,
}

/// Deterministic JSON-Schema bundle (stable field order — pitfall #2).
pub fn export_schema_json() -> String {
    let mut map: BTreeMap<&str, schemars::schema::RootSchema> = BTreeMap::new();
    map.insert("Item", schemars::schema_for!(Item));
    map.insert("InitializeParams", schemars::schema_for!(InitializeParams));
    map.insert("ThreadStartParams", schemars::schema_for!(ThreadStartParams));
    map.insert("TurnStartParams", schemars::schema_for!(TurnStartParams));
    map.insert("CaptureParams", schemars::schema_for!(CaptureParams));
    map.insert("CaptureResult", schemars::schema_for!(CaptureResult));
    map.insert("CaptureDetail", schemars::schema_for!(CaptureDetail));
    map.insert("CapturePatch", schemars::schema_for!(CapturePatch));
    map.insert("TurnResult", schemars::schema_for!(TurnResult));
    map.insert("ConversationSummary", schemars::schema_for!(protocol::ConversationSummary));
    map.insert("ConversationMessage", schemars::schema_for!(protocol::ConversationMessage));
    map.insert("IngestRunResult", schemars::schema_for!(IngestRunResult));
    map.insert("AppReleaseManifest", schemars::schema_for!(AppReleaseManifest));
    serde_json::to_string_pretty(&map).unwrap_or_default()
}
