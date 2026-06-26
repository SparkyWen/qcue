// QCue S1-R23/R24 — SSE byte stream → `data:` frames; 10MB cap; unknown lines pass through as frames.
#![allow(clippy::unwrap_used)]
use futures_util::{stream, StreamExt};
use http::sse::{sse_frames, SseFrame, MAX_SSE_BUFFER};

#[tokio::test]
async fn test_sse_splits_data_frames() {
    let bytes = stream::iter(vec![
        Ok::<_, std::io::Error>(bytes::Bytes::from("data: {\"a\":1}\n\n")),
        Ok(bytes::Bytes::from("event: ping\ndata: {\"b\":2}\n\n")),
        Ok(bytes::Bytes::from("data: [DONE]\n\n")),
    ]);
    let frames: Vec<SseFrame> = sse_frames(Box::pin(bytes))
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(frames[0].data, "{\"a\":1}");
    assert_eq!(frames[1].event.as_deref(), Some("ping"));
    assert_eq!(frames[1].data, "{\"b\":2}");
    assert_eq!(frames[2].data, "[DONE]");
}

#[tokio::test]
async fn test_sse_buffer_cap() {
    // A single oversized frame with no terminator aborts rather than buffering unboundedly.
    let huge = bytes::Bytes::from(vec![b'x'; MAX_SSE_BUFFER + 10]);
    let s = stream::iter(vec![Ok::<_, std::io::Error>(huge)]);
    let res: Vec<_> = sse_frames(Box::pin(s)).collect::<Vec<_>>().await;
    assert!(res.iter().any(|r| r.is_err()), "oversized buffer must error");
}
