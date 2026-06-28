#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R79..R82 — the OpenAI cloud STT provider: happy-path parse, non-2xx → envelope, and
// transport failure → envelope (never raises). Driven against a wiremock server.
use router::stt::TranscriptionProvider;
use router::stt_openai::{audio_head_hex, detect_audio_format, OpenAiTranscriptionProvider};
use wiremock::matchers::{body_string_contains, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Minimal but realistic container heads so the sniffer (and OpenAI's extension detection) see what
/// the device actually sends.
fn m4a_head() -> Vec<u8> {
    // `....ftypM4A ` — the AAC-in-MP4 head AVAudioRecorder writes.
    let mut v = vec![0x00, 0x00, 0x00, 0x20];
    v.extend_from_slice(b"ftypM4A ");
    v.extend_from_slice(&[0u8; 16]);
    v
}
fn wav_head() -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&[0x24, 0x08, 0x00, 0x00]);
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&[0u8; 16]);
    v
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

#[tokio::test]
async fn openai_transcribe_sends_default_model_and_parses_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .and(header_exists("authorization"))
        // the resolved default model is carried in the multipart form body
        .and(body_string_contains("gpt-4o-mini-transcribe-2025-12-15"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "你好世界"
        })))
        .mount(&server)
        .await;

    let provider =
        OpenAiTranscriptionProvider::new(client(), "sk-test".into()).with_base_url(server.uri());
    let r = provider.transcribe(b"fake-audio-bytes", None, Some("zh")).await;
    assert!(r.success, "error: {:?}", r.error);
    assert_eq!(r.transcript, "你好世界");
    assert_eq!(r.provider, "openai");
}

#[test]
fn default_model_is_the_pinned_mini_snapshot() {
    let p = OpenAiTranscriptionProvider::new(client(), "k".into());
    assert_eq!(p.default_model(), Some("gpt-4o-mini-transcribe-2025-12-15"));
}

#[test]
fn with_model_overrides_the_default() {
    let p = OpenAiTranscriptionProvider::new(client(), "k".into()).with_model("custom-stt-model");
    assert_eq!(p.default_model(), Some("custom-stt-model"));
}

#[tokio::test]
async fn openai_transcribe_non_2xx_is_an_envelope_not_a_panic() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("invalid api key"))
        .mount(&server)
        .await;

    let provider =
        OpenAiTranscriptionProvider::new(client(), "sk-bad".into()).with_base_url(server.uri());
    let r = provider.transcribe(b"audio", None, None).await;
    assert!(!r.success);
    assert!(r.error.unwrap().contains("401"));
}

#[tokio::test]
async fn openai_transcribe_transport_failure_is_an_envelope() {
    // An unroutable port → connection error → envelope, never a raise (S1-R79).
    let provider = OpenAiTranscriptionProvider::new(client(), "sk".into())
        .with_base_url("http://127.0.0.1:1");
    let r = provider.transcribe(b"audio", None, None).await;
    assert!(!r.success);
    assert!(r.error.unwrap().to_lowercase().contains("transport"));
}

/// Live end-to-end against the real OpenAI API — ignored by default (needs network + a real key).
/// Run with: `QCUE_LIVE_OPENAI_KEY=sk-... QCUE_LIVE_AUDIO=/path/clip.m4a \
///   cargo test -p router --test stt_openai live_openai -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn live_openai_transcribes_a_real_clip() {
    let key = std::env::var("QCUE_LIVE_OPENAI_KEY").expect("set QCUE_LIVE_OPENAI_KEY");
    let audio_path = std::env::var("QCUE_LIVE_AUDIO").expect("set QCUE_LIVE_AUDIO");
    let audio = std::fs::read(&audio_path).expect("read audio file");
    let fmt = detect_audio_format(&audio);
    eprintln!("live: {} bytes, detected {} ({})", audio.len(), fmt.kind, fmt.file_name);

    let provider = OpenAiTranscriptionProvider::new(client(), key);
    let r = provider.transcribe(&audio, None, None).await;
    eprintln!("live result: success={} transcript={:?} error={:?}", r.success, r.transcript, r.error);
    assert!(r.success, "live transcription failed: {:?}", r.error);
    assert!(!r.transcript.trim().is_empty(), "expected a non-empty transcript");
}

#[test]
fn detect_audio_format_sniffs_the_container_from_magic_bytes() {
    assert_eq!(detect_audio_format(&m4a_head()).file_name, "audio.m4a");
    assert_eq!(detect_audio_format(&m4a_head()).mime, "audio/mp4");
    assert_eq!(detect_audio_format(&wav_head()).file_name, "audio.wav");
    assert_eq!(detect_audio_format(&wav_head()).mime, "audio/wav");
    assert_eq!(detect_audio_format(b"OggS\0\0\0\0").kind, "ogg");
    assert_eq!(detect_audio_format(&[0x1A, 0x45, 0xDF, 0xA3, 0, 0]).kind, "webm");
    assert_eq!(detect_audio_format(b"fLaC\0\0").kind, "flac");
    assert_eq!(detect_audio_format(b"caff\0\0").kind, "caf");
    assert_eq!(detect_audio_format(b"ID3\x04\0\0").kind, "mp3");
    // a 3GP brand
    let mut g = vec![0, 0, 0, 0x18];
    g.extend_from_slice(b"ftyp3gp4");
    assert_eq!(detect_audio_format(&g).kind, "3gp");
    // unknown / too-short bytes fall back to the app's m4a recording format
    assert_eq!(detect_audio_format(b"fake-audio-bytes").file_name, "audio.m4a");
    assert_eq!(detect_audio_format(b"").file_name, "audio.m4a");
}

#[test]
fn audio_head_hex_is_a_redaction_safe_prefix() {
    assert_eq!(audio_head_hex(&[0x00, 0x00, 0x00, 0x20, 0x66], 4), "00000020");
    assert_eq!(audio_head_hex(b"AB", 16), "4142"); // shorter than n → whole buffer
    assert_eq!(audio_head_hex(b"", 16), "");
}

#[tokio::test]
async fn openai_upload_is_labeled_with_the_sniffed_extension_not_hardcoded_m4a() {
    // The multipart MUST carry the *real* container's filename — OpenAI detects format from it. WAV
    // bytes labeled `audio.wav`, not the old hardcoded `audio.m4a`.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .and(body_string_contains("filename=\"audio.wav\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "ok"})))
        .mount(&server)
        .await;

    let provider =
        OpenAiTranscriptionProvider::new(client(), "sk".into()).with_base_url(server.uri());
    let r = provider.transcribe(&wav_head(), None, None).await;
    assert!(r.success, "WAV bytes must be uploaded as audio.wav; error: {:?}", r.error);
}

#[tokio::test]
async fn openai_corrupted_400_becomes_an_actionable_message_with_size_and_format() {
    // The exact OpenAI rejection the user hit → a human, actionable line (shown verbatim by the app),
    // carrying the byte size + detected container so the cause is diagnosable from the message alone.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": {
                "message": "Audio file might be corrupted or unsupported",
                "type": "invalid_request_error",
                "param": "file",
                "code": "invalid_value"
            }
        })))
        .mount(&server)
        .await;

    let provider =
        OpenAiTranscriptionProvider::new(client(), "sk".into()).with_base_url(server.uri());
    let r = provider.transcribe(&m4a_head(), None, None).await;
    assert!(!r.success);
    let err = r.error.unwrap();
    assert!(err.contains("too short"), "should be actionable: {err}");
    assert!(err.contains("28 bytes"), "should carry the size: {err}"); // m4a_head() is 28 bytes
    assert!(err.contains("m4a"), "should carry the detected format: {err}");
    // and it must NOT leak the raw OpenAI JSON / status code at the user
    assert!(!err.contains("invalid_value"), "raw provider JSON must not surface: {err}");
}

#[tokio::test]
async fn provider_name_override_is_reflected_in_the_envelope() {
    // Groq/Zhipu reuse this provider via base_url+model; the envelope must report THEIR id, not "openai".
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "hi"})))
        .mount(&server)
        .await;
    let p = OpenAiTranscriptionProvider::new(client(), "k".into())
        .with_base_url(server.uri())
        .with_provider_name("zhipu")
        .with_model("glm-asr-2512");
    let r = p.transcribe(&wav_head(), None, None).await;
    assert!(r.success, "error: {:?}", r.error);
    assert_eq!(r.provider, "zhipu");
    assert_eq!(p.name(), "zhipu");
}
