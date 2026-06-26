#![allow(clippy::unwrap_used, clippy::expect_used)]
use app_server_protocol::*;
use uuid::Uuid;

#[test]
fn test_thread_turn_item_shapes() {
    // every Item variant round-trips serde under the camelCase tagged contract (S3-R36)
    let items = vec![
        Item::IdeaCaptured { idea_id: Uuid::now_v7(), body: "x".into() },
        Item::VoiceTranscript { idea_id: Uuid::now_v7(), text: "t".into(), provider: "stub".into() },
        Item::WikiEdit { page_id: Uuid::now_v7(), slug: "s".into(), op: WikiEditOp::Update },
        Item::RecallResult { answer_delta: "a".into(), citations: vec![Citation{ rel_path: "sources/x.md".into(), start_line: 1, end_line: 4 }] },
        Item::AgentMessage { delta: "hi".into() },
        Item::DreamTurn { phase: DreamPhase::Consolidate, pages_touched: vec![Uuid::now_v7()] },
        Item::Reasoning { delta: "think".into() },
        Item::Error { code: -32001, message: "overloaded".into() },
    ];
    for it in items {
        let s = serde_json::to_string(&it).unwrap();
        let back: Item = serde_json::from_str(&s).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), s);
        assert!(s.contains("\"type\":"), "Item must be internally tagged with `type`");
    }
}

#[test]
fn test_jsonrpc_lite_envelope() {
    // a request WITHOUT the "jsonrpc" field is accepted (S3-R33)
    let raw = r#"{"id":1,"method":"turn/start","params":{}}"#;
    let m: Message = serde_json::from_str(raw).unwrap();
    assert!(matches!(m, Message::Request(_)));
    let n: Message = serde_json::from_str(r#"{"method":"item/delta","params":{}}"#).unwrap();
    assert!(matches!(n, Message::Notification(_)));
    assert_eq!(error_codes::OVERLOADED, -32001);
}

#[test]
fn test_envelope_seq_monotonic_shape() {
    // `event` is a forward-compat String on the wire; construct via the typed helper's canonical token.
    let env = RuntimeEventEnvelope {
        schema_version: 1, thread_id: Uuid::now_v7(), turn_id: None, seq: 7,
        event: RuntimeEvent::TurnStarted.as_wire().to_string(), payload: serde_json::json!({}),
    };
    let s = serde_json::to_string(&env).unwrap();
    let back: RuntimeEventEnvelope = serde_json::from_str(&s).unwrap();
    assert_eq!(back.seq, 7);
    assert_eq!(back.schema_version, 1);
    // unknown/future event kinds MUST deserialize (no deny_unknown_fields; `event: String`).
    let fwd: RuntimeEventEnvelope = serde_json::from_str(
        r#"{"schema_version":1,"thread_id":"00000000-0000-7000-8000-000000000000","seq":8,"event":"someFutureKind","payload":null}"#
    ).unwrap();
    assert_eq!(fwd.event, "someFutureKind");
}

#[test]
fn test_v1_v2_coexist() {
    use app_server_protocol::{v1, v2};
    // a v1 ThreadStart and a v2 ThreadStart both deserialize their own shape (S3-R43)
    let p1: v1::ThreadStartParams = serde_json::from_str(r#"{"kind":"recall"}"#).unwrap();
    assert_eq!(p1.kind, "recall");
    let p2: v2::ThreadStartParams = serde_json::from_str(r#"{"kind":"recall","labels":["x"]}"#).unwrap();
    assert_eq!(p2.labels, vec!["x".to_string()]);
    // forward-compat: the envelope carries `event` as a plain String (known or future kind), and an omitted `payload` defaults to null.
    let env: app_server_protocol::RuntimeEventEnvelope =
        serde_json::from_str(r#"{"schema_version":1,"thread_id":"00000000-0000-7000-8000-000000000000","seq":1,"event":"itemDelta"}"#).unwrap();
    assert_eq!(env.event, "itemDelta");
    assert!(env.payload.is_null());
}

#[test]
fn test_schema_export_stable() {
    // deterministic: two exports are byte-identical (stable order via BTreeMap, pitfall #2)
    let a = app_server_protocol::v1::export_schema_json();
    let b = app_server_protocol::v1::export_schema_json();
    assert_eq!(a, b, "schema export must be deterministic");
    assert!(a.contains("\"Item\""));
}

#[test]
fn test_codegen_artifact_matches() {
    // CI fails if the checked-in artifact drifts from a fresh export (S3-R42).
    let fresh = app_server_protocol::v1::export_schema_json();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../codegen/schema.v1.json");
    if let Ok(checked) = std::fs::read_to_string(&path) {
        assert_eq!(fresh.trim(), checked.trim(), "run `cargo run -p app-server-protocol --bin export-schema`");
    }
}

#[test]
fn test_conversation_dtos_roundtrip_and_export() {
    use protocol::{ConversationMessage, ConversationSummary};
    let s = ConversationSummary {
        id: Uuid::now_v7(),
        title: "Postgres migration".into(),
        updated_at: "2026-06-16T00:00:00Z".into(),
        last_snippet: Some("…partial indexes".into()),
    };
    let js = serde_json::to_string(&s).unwrap();
    let back: ConversationSummary = serde_json::from_str(&js).unwrap();
    assert_eq!(serde_json::to_string(&back).unwrap(), js);

    let m = ConversationMessage { role: "assistant".into(), content: "hi".into() };
    let jm = serde_json::to_string(&m).unwrap();
    let back2: ConversationMessage = serde_json::from_str(&jm).unwrap();
    assert_eq!(serde_json::to_string(&back2).unwrap(), jm);

    // the exported schema bundle now includes both DTOs (REC-R6).
    let bundle = app_server_protocol::v1::export_schema_json();
    assert!(bundle.contains("ConversationSummary"));
    assert!(bundle.contains("ConversationMessage"));
}

#[test]
fn test_ingest_run_result_roundtrips_and_is_exported() {
    use app_server_protocol::v1::IngestRunResult;
    let r = IngestRunResult { enqueued: 2, job_ids: vec![Uuid::now_v7(), Uuid::now_v7()] };
    let s = serde_json::to_string(&r).unwrap();
    let back: IngestRunResult = serde_json::from_str(&s).unwrap();
    assert_eq!(back.enqueued, 2);
    assert_eq!(back.job_ids.len(), 2);
    // it is registered in the exported schema bundle (so the Dart codegen + drift test cover it).
    assert!(app_server_protocol::v1::export_schema_json().contains("\"IngestRunResult\""));
}

#[test]
fn test_app_release_manifest_roundtrips_and_is_exported() {
    use app_server_protocol::v1::AppReleaseManifest;
    let m = AppReleaseManifest {
        platform: "android".into(),
        latest_build: 10,
        latest_version: "1.0.4".into(),
        min_supported_build: 9,
        changelog: "Bug fixes.".into(),
        android_apk_path: Some("/v1/app/apk/10".into()),
        android_apk_sha256: Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into()),
        ios_app_store_url: None,
        published_at: "2026-06-24T00:00:00Z".into(),
    };
    let s = serde_json::to_string(&m).unwrap();
    let back: AppReleaseManifest = serde_json::from_str(&s).unwrap();
    assert_eq!(back.latest_build, 10);
    assert_eq!(back.min_supported_build, 9);
    // The integrity hash must survive the wire round-trip so the client can verify the APK download.
    assert_eq!(back.android_apk_sha256.as_deref(), Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
    // A manifest serialized WITHOUT the field still deserializes (back-compat: #[serde(default)] ⇒ None).
    let legacy = r#"{"platform":"android","latest_build":1,"latest_version":"1","min_supported_build":1,"changelog":"","android_apk_path":null,"ios_app_store_url":null,"published_at":""}"#;
    let parsed: AppReleaseManifest = serde_json::from_str(legacy).unwrap();
    assert!(parsed.android_apk_sha256.is_none());
    // registered in the exported schema bundle (so the drift test + Dart codegen cover it).
    assert!(app_server_protocol::v1::export_schema_json().contains("\"AppReleaseManifest\""));
}
