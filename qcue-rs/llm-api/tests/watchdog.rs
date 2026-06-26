// QCue S1-R23 — per-provider idle + TTFB watchdog on EVERY provider stream.
#![allow(clippy::unwrap_used)]
use futures_util::{stream, StreamExt};
use llm_api::watchdog::{with_watchdog, WatchdogCfg};
use protocol::{ApiError, StreamEvent};
use std::time::Duration;

#[tokio::test]
async fn test_idle_watchdog_fires_after_first_event() {
    // emit MessageStart, then hang forever → StreamIdle after the idle window.
    let inner = stream::unfold(0u8, |n| async move {
        match n {
            0 => Some((Ok::<StreamEvent, ApiError>(StreamEvent::MessageStart), 1u8)),
            _ => {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                None
            }
        }
    });
    let cfg = WatchdogCfg { idle_ms: 50, ttfb_ms: 1000 };
    let mut s = with_watchdog(Box::pin(inner), cfg);
    let first = s.next().await.unwrap();
    assert!(matches!(first, Ok(StreamEvent::MessageStart)));
    let second = s.next().await.unwrap();
    assert!(matches!(second, Err(ApiError::StreamIdle { .. })), "expected idle, got {second:?}");
}

#[tokio::test]
async fn test_ttfb_watchdog_fires_before_first_byte() {
    // never sends the first event → TTFB timeout.
    let inner = stream::unfold((), |_| async {
        tokio::time::sleep(Duration::from_secs(3600)).await;
        None::<(Result<StreamEvent, ApiError>, ())>
    });
    let cfg = WatchdogCfg { idle_ms: 1000, ttfb_ms: 50 };
    let mut s = with_watchdog(Box::pin(inner), cfg);
    let first = s.next().await.unwrap();
    assert!(matches!(first, Err(ApiError::StreamTtfb { .. })), "expected ttfb, got {first:?}");
}

#[tokio::test]
async fn test_no_content_lost_when_healthy() {
    let inner = stream::iter(vec![
        Ok::<StreamEvent, ApiError>(StreamEvent::MessageStart),
        Ok(StreamEvent::MessageStop),
    ]);
    let cfg = WatchdogCfg { idle_ms: 1000, ttfb_ms: 1000 };
    let evs: Vec<_> = with_watchdog(Box::pin(inner), cfg).collect::<Vec<_>>().await;
    assert_eq!(evs.len(), 2);
    assert!(evs.iter().all(|e| e.is_ok()));
}
