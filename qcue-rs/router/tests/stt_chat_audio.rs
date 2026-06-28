#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R79..R82 — Family B chat-completions `input_audio` STT (Qwen + Gemini): happy-path parse,
// non-2xx → envelope, transport failure → envelope. Driven against a wiremock server.
use router::stt::TranscriptionProvider;
use router::stt_chat_audio::ChatAudioTranscriptionProvider;
use wiremock::matchers::{body_string_contains, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

#[tokio::test]
async fn chat_audio_posts_input_audio_and_parses_message_content() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header_exists("authorization"))
        .and(body_string_contains("input_audio"))
        .and(body_string_contains("qwen3-asr-flash"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "你好世界"}}]
        })))
        .mount(&server)
        .await;
    let p = ChatAudioTranscriptionProvider::new(
        client(), "k".into(), server.uri(), "qwen3-asr-flash", "qwen");
    let r = p.transcribe(b"RIFF\0\0\0\0WAVEfmt ", None, Some("zh")).await;
    assert!(r.success, "error: {:?}", r.error);
    assert_eq!(r.transcript, "你好世界");
    assert_eq!(r.provider, "qwen");
}

#[tokio::test]
async fn chat_audio_non_2xx_is_an_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .mount(&server)
        .await;
    let p = ChatAudioTranscriptionProvider::new(
        client(), "k".into(), server.uri(), "gemini-2.5-flash", "gemini");
    let r = p.transcribe(b"audio", None, None).await;
    assert!(!r.success);
    assert!(r.error.unwrap().contains("429"));
}

#[tokio::test]
async fn chat_audio_transport_failure_is_an_envelope() {
    let p = ChatAudioTranscriptionProvider::new(
        client(), "k".into(), "http://127.0.0.1:1", "m", "qwen");
    let r = p.transcribe(b"audio", None, None).await;
    assert!(!r.success);
    assert!(r.error.unwrap().to_lowercase().contains("transport"));
}

#[tokio::test]
async fn chat_audio_empty_content_is_an_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": ""}}]
        })))
        .mount(&server)
        .await;
    let p = ChatAudioTranscriptionProvider::new(
        client(), "k".into(), server.uri(), "qwen3-asr-flash", "qwen");
    let r = p.transcribe(b"audio", None, None).await;
    assert!(!r.success);
    assert!(r.error.unwrap().to_lowercase().contains("empty"));
}

#[tokio::test]
async fn chat_audio_non_json_2xx_body_is_an_envelope() {
    // A 200 with a non-JSON body must fold to the decode envelope, never panic (S1-R79).
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
        .mount(&server)
        .await;
    let p = ChatAudioTranscriptionProvider::new(
        client(), "k".into(), server.uri(), "qwen3-asr-flash", "qwen");
    let r = p.transcribe(b"audio", None, None).await;
    assert!(!r.success);
    assert!(r.error.unwrap().to_lowercase().contains("decode"));
}

// Live validation of the real `input_audio` wire shape. Ignored by default.
// QCUE_LIVE_QWEN_KEY=sk-... QCUE_LIVE_AUDIO=/path/clip.m4a \
//   cargo test -p router --test stt_chat_audio live_qwen -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn live_qwen_transcribes_a_real_clip() {
    let key = std::env::var("QCUE_LIVE_QWEN_KEY").expect("set QCUE_LIVE_QWEN_KEY");
    let audio = std::fs::read(std::env::var("QCUE_LIVE_AUDIO").expect("set QCUE_LIVE_AUDIO")).unwrap();
    let p = ChatAudioTranscriptionProvider::new(
        client(), key, "https://dashscope.aliyuncs.com/compatible-mode/v1", "qwen3-asr-flash", "qwen");
    let r = p.transcribe(&audio, None, None).await;
    eprintln!("qwen: success={} transcript={:?} error={:?}", r.success, r.transcript, r.error);
    assert!(r.success, "{:?}", r.error);
}

#[tokio::test]
#[ignore]
async fn live_gemini_transcribes_a_real_clip() {
    let key = std::env::var("QCUE_LIVE_GEMINI_KEY").expect("set QCUE_LIVE_GEMINI_KEY");
    let audio = std::fs::read(std::env::var("QCUE_LIVE_AUDIO").expect("set QCUE_LIVE_AUDIO")).unwrap();
    let p = ChatAudioTranscriptionProvider::new(
        client(), key, "https://generativelanguage.googleapis.com/v1beta/openai", "gemini-2.5-flash", "gemini");
    let r = p.transcribe(&audio, None, None).await;
    eprintln!("gemini: success={} transcript={:?} error={:?}", r.success, r.transcript, r.error);
    assert!(r.success, "{:?}", r.error);
}
