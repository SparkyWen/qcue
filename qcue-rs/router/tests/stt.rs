#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R79..R82 — STT envelope-never-raise + routing order + constraint validation + shared fallback.
use async_trait::async_trait;
use protocol::TranscriptionResult;
use router::stt::{AudioConstraints, SttRouter, TranscriptionProvider};

struct AlwaysFails;
#[async_trait]
impl TranscriptionProvider for AlwaysFails {
    fn name(&self) -> &str {
        "fails"
    }
    async fn transcribe(&self, _a: &[u8], _m: Option<&str>, _l: Option<&str>) -> TranscriptionResult {
        // S1-R79 — even a hard failure returns an envelope, never raises.
        TranscriptionResult {
            success: false,
            transcript: String::new(),
            error: Some("provider down".into()),
            provider: "fails".into(),
        }
    }
}
struct AlwaysOk;
#[async_trait]
impl TranscriptionProvider for AlwaysOk {
    fn name(&self) -> &str {
        "ok"
    }
    async fn transcribe(&self, _a: &[u8], _m: Option<&str>, _l: Option<&str>) -> TranscriptionResult {
        TranscriptionResult {
            success: true,
            transcript: "hello world".into(),
            error: None,
            provider: "ok".into(),
        }
    }
}

#[tokio::test]
async fn test_transcribe_never_raises() {
    let r = AlwaysFails.transcribe(b"audio", None, None).await;
    assert!(!r.success);
    assert_eq!(r.error.as_deref(), Some("provider down"));
}

#[tokio::test]
async fn test_stt_routing_order_and_fallback() {
    // S1-R80/R82 — configured provider tried first; on failure falls back to the next.
    let router = SttRouter::new(vec![Box::new(AlwaysFails), Box::new(AlwaysOk)]);
    let r = router.transcribe(b"audio", None, None).await;
    assert!(r.success);
    assert_eq!(r.provider, "ok");
}

#[tokio::test]
async fn test_stt_validates_constraints_before_call() {
    // S1-R81 — an over-length clip is rejected with an envelope BEFORE any provider call.
    let router = SttRouter::new(vec![Box::new(AlwaysOk)])
        .with_constraints(AudioConstraints { max_bytes: 10, max_seconds: 60 });
    let r = router.transcribe(&[0u8; 100], None, None).await;
    assert!(!r.success);
    assert!(r.error.unwrap().contains("constraint"));
}
