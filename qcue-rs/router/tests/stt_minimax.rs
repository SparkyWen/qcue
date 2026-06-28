#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R79..R82 — MiniMax vendor-native JSON ASR: auth + GroupId + composite-credential mechanics
// (shape-stable), envelope-never-raise. The request BODY keys / ASR PATH / RESPONSE path are an
// ASSUMED shape pending console-doc confirmation (see stt_minimax.rs header + the #[ignore] live test).
use router::stt::TranscriptionProvider;
use router::stt_minimax::MiniMaxTranscriptionProvider;
use wiremock::matchers::{header_exists, method, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client() -> reqwest::Client {
    reqwest::Client::new()
}
fn blob() -> String {
    r#"{"api_key":"k","group_id":"g123"}"#.into()
}

#[tokio::test]
async fn minimax_sends_groupid_and_parses_transcript() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header_exists("authorization"))
        .and(query_param("GroupId", "g123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "你好"})))
        .mount(&server)
        .await;
    let p = MiniMaxTranscriptionProvider::new(client(), blob(), server.uri(), "");
    let r = p.transcribe(b"RIFF\0\0\0\0WAVEfmt ", None, None).await;
    assert!(r.success, "error: {:?}", r.error);
    assert_eq!(r.transcript, "你好");
    assert_eq!(r.provider, "minimax");
}

#[tokio::test]
async fn minimax_missing_group_id_is_an_envelope() {
    let p = MiniMaxTranscriptionProvider::new(
        client(), r#"{"api_key":"k"}"#.into(), "http://127.0.0.1:1", "");
    let r = p.transcribe(b"audio", None, None).await;
    assert!(!r.success);
    assert!(r.error.unwrap().to_lowercase().contains("group"));
}

#[tokio::test]
async fn minimax_non_2xx_is_an_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(query_param("GroupId", "g123"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;
    let p = MiniMaxTranscriptionProvider::new(client(), blob(), server.uri(), "");
    let r = p.transcribe(b"audio", None, None).await;
    assert!(!r.success);
    assert!(r.error.unwrap().contains("401"));
}

#[tokio::test]
async fn minimax_bare_string_credential_is_missing_groupid_envelope() {
    // A non-JSON credential (legacy bare key) degrades to the actionable "missing GroupId" envelope.
    let p = MiniMaxTranscriptionProvider::new(client(), "sk-abc".into(), "http://127.0.0.1:1", "");
    let r = p.transcribe(b"audio", None, None).await;
    assert!(!r.success);
    assert!(r.error.unwrap().to_lowercase().contains("group"));
}

#[tokio::test]
async fn minimax_empty_api_key_is_an_envelope() {
    let p = MiniMaxTranscriptionProvider::new(
        client(), r#"{"api_key":"","group_id":"g"}"#.into(), "http://127.0.0.1:1", "");
    let r = p.transcribe(b"audio", None, None).await;
    assert!(!r.success);
    assert!(r.error.unwrap().to_lowercase().contains("api_key"));
}

// Live validation of the real MiniMax ASR wire shape. Ignored by default. CONFIRMS the assumed body/
// path/response in stt_minimax.rs against the real API.
// QCUE_LIVE_MINIMAX_KEY=... QCUE_LIVE_MINIMAX_GROUP=... QCUE_LIVE_AUDIO=/path/clip.m4a \
//   cargo test -p router --test stt_minimax live_minimax -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn live_minimax_transcribes_a_real_clip() {
    let key = std::env::var("QCUE_LIVE_MINIMAX_KEY").expect("set QCUE_LIVE_MINIMAX_KEY");
    let gid = std::env::var("QCUE_LIVE_MINIMAX_GROUP").expect("set QCUE_LIVE_MINIMAX_GROUP");
    let audio = std::fs::read(std::env::var("QCUE_LIVE_AUDIO").expect("set QCUE_LIVE_AUDIO")).unwrap();
    let blob = serde_json::json!({"api_key": key, "group_id": gid}).to_string();
    let p = MiniMaxTranscriptionProvider::new(client(), blob, "https://api.minimax.io/v1", "");
    let r = p.transcribe(&audio, None, None).await;
    eprintln!("minimax: success={} transcript={:?} error={:?}", r.success, r.transcript, r.error);
    assert!(r.success, "{:?}", r.error);
}
