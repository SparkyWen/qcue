#![allow(clippy::unwrap_used, clippy::expect_used)]
use app_server_protocol::v1::{CaptureParams, CapturePatch};

#[test]
fn capture_params_accepts_location_and_time() {
    let j = r#"{"kind":"text","body":"hi","origin":"capture","captured_at":"2026-06-18T10:00:00Z","lat":31.2,"lng":121.4,"loc_accuracy_m":9.0}"#;
    let p: CaptureParams = serde_json::from_str(j).unwrap();
    assert_eq!(p.lat, Some(31.2));
    assert_eq!(p.captured_at.as_deref(), Some("2026-06-18T10:00:00Z"));
}

#[test]
fn capture_params_without_location_still_parses() {
    let p: CaptureParams = serde_json::from_str(r#"{"kind":"text","body":"hi","origin":"capture"}"#).unwrap();
    assert_eq!(p.lat, None);
    assert_eq!(p.captured_at, None);
}

#[test]
fn capture_patch_rejects_unknown_fields() {
    assert!(serde_json::from_str::<CapturePatch>(r#"{"bogus":1}"#).is_err());
    let patch: CapturePatch = serde_json::from_str(r#"{"body":"fixed"}"#).unwrap();
    assert_eq!(patch.body.as_deref(), Some("fixed"));
}
