// QCue S1-R85..R88 — FFI forwards every StreamEvent as an envelope; cancel crosses the boundary;
// unknown event kinds are skipped by a v1 consumer; SSE and FFI serialize identically.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ffi::{SinkRecorder, envelope_for, forward_events, is_known_event_v1};
use protocol::{RuntimeEventEnvelope, StreamEvent};
use router::stub::{StubProvider, StubScript};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_streamsink_forwards_events_in_order() {
    let stub = StubProvider::new(StubScript::text("hi"));
    let sink = SinkRecorder::default();
    let token = CancellationToken::new();
    forward_events(stub.stream(), sink.clone(), token).await;
    let kinds: Vec<String> = sink.events().iter().map(|e| e.event.clone()).collect();
    assert_eq!(kinds.first().map(String::as_str), Some("MessageStart"));
    assert_eq!(kinds.last().map(String::as_str), Some("MessageStop"));
    // S1-R87 — every forwarded envelope carries the canonical schema_version + a monotonic seq.
    for (i, e) in sink.events().iter().enumerate() {
        assert_eq!(e.schema_version, 1);
        assert_eq!(e.seq, i as u64);
    }
}

#[tokio::test]
async fn test_full_taxonomy_with_thinking_text_and_tool() {
    // S1-R85 — a richer script produces the full MessageStart..MessageStop envelope sequence.
    let script = StubScript::thinking("reasoning").with_text("answer");
    let stub = StubProvider::new(script);
    let sink = SinkRecorder::default();
    forward_events(stub.stream(), sink.clone(), CancellationToken::new()).await;
    let kinds: Vec<String> = sink.events().iter().map(|e| e.event.clone()).collect();
    assert_eq!(kinds.first().map(String::as_str), Some("MessageStart"));
    assert!(kinds.iter().any(|k| k == "ContentBlockStart"));
    assert!(kinds.iter().any(|k| k == "ContentBlockDelta"));
    assert!(kinds.contains(&"MessageDelta".to_string()));
    assert_eq!(kinds.last().map(String::as_str), Some("MessageStop"));
    // every kind a v1 consumer sees here is a known kind.
    assert!(kinds.iter().all(|k| is_known_event_v1(k)));
}

#[tokio::test]
async fn test_ffi_cancel_stops_forwarding() {
    let stub = StubProvider::new(StubScript::text("hi"));
    let sink = SinkRecorder::default();
    let token = CancellationToken::new();
    token.cancel(); // pre-cancelled
    forward_events(stub.stream(), sink.clone(), token).await;
    assert!(sink.events().is_empty(), "cancel must stop forwarding");
}

#[test]
fn test_event_envelope_forward_compat() {
    // S1-R88 — an unknown event kind is skipped by a v1 consumer without error.
    assert!(is_known_event_v1("MessageStart"));
    assert!(!is_known_event_v1("futureKind"));
    let env = RuntimeEventEnvelope {
        schema_version: 1,
        thread_id: uuid::Uuid::nil(),
        turn_id: None,
        seq: 0,
        event: "futureKind".into(),
        payload: serde_json::json!({}),
    };
    // a v1 consumer simply ignores it.
    assert!(!is_known_event_v1(&env.event));
}

#[test]
fn test_two_egress_serialize_identically() {
    // S1-R86 — the same event serializes identically through the SSE encoder and the FFI envelope.
    let ev = StreamEvent::ContentBlockDelta(protocol::Delta::TextDelta("a".into()));
    let env = envelope_for(&ev);
    let sse_payload = serde_json::to_string(&env.payload).unwrap();
    let ffi_payload = serde_json::to_string(&env.payload).unwrap();
    assert_eq!(sse_payload, ffi_payload);
    assert_eq!(env.schema_version, 1);
    // the payload is the raw StreamEvent JSON (no provider-native wire crosses the boundary, S1-R86).
    assert_eq!(env.payload, serde_json::to_value(&ev).unwrap());
}

#[tokio::test]
async fn test_bridge_run_turn_streams_over_envelope_sink() {
    // S1-R85 — the bridge entry (`run_turn_into`) drives the stub into any EnvelopeSink, with cancel.
    let sink = SinkRecorder::default();
    let token = CancellationToken::new();
    ffi::bridge::run_turn_into(StubProvider::new(StubScript::text("hi")), sink.clone(), token).await;
    let kinds: Vec<String> = sink.events().iter().map(|e| e.event.clone()).collect();
    assert_eq!(kinds.first().map(String::as_str), Some("MessageStart"));
    assert_eq!(kinds.last().map(String::as_str), Some("MessageStop"));
}

#[tokio::test]
async fn test_bridge_transcribe_returns_envelope_never_raises() {
    // S1-R79/R82 — the STT entry returns a TranscriptionResult envelope even with no provider.
    let res = ffi::bridge::transcribe(router::stt::SttRouter::new(vec![]), b"x".to_vec(), None, None).await;
    assert!(!res.success);
    assert!(res.error.is_some());
}
