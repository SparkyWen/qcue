// QCue S1-R23 — wrap a StreamEventBox with an idle + TTFB watchdog. On timeout, drop inner (Tokio cancels).
use futures_util::StreamExt;
use protocol::{ApiError, StreamEventBox};
use std::time::Duration;
use tokio::time::timeout;

#[derive(Clone, Copy, Debug)]
pub struct WatchdogCfg {
    pub idle_ms: u64,
    pub ttfb_ms: u64,
}

// Plan deviation (real toolchain): the plan returns `impl Stream + Send`, but `async_stream`'s
// output is `!Unpin` and the watchdog test calls `.next()` on an unpinned binding — which requires
// `Unpin`. Returning the already-boxed `StreamEventBox` (a `Pin<Box<..>>`, which IS `Unpin`) keeps
// every call site in the plan's test working unchanged. Same event semantics.
pub fn with_watchdog(inner: StreamEventBox, cfg: WatchdogCfg) -> StreamEventBox {
    Box::pin(async_stream::stream! {
        let mut inner = inner;
        let mut got_first = false;
        loop {
            let window = if got_first { cfg.idle_ms } else { cfg.ttfb_ms };
            match timeout(Duration::from_millis(window), inner.next()).await {
                Ok(Some(item)) => { got_first = true; yield item; }
                Ok(None) => break, // stream ended cleanly
                Err(_) => {
                    if got_first { yield Err(ApiError::StreamIdle { idle_ms: cfg.idle_ms }); }
                    else { yield Err(ApiError::StreamTtfb { ttfb_ms: cfg.ttfb_ms }); }
                    return; // drop inner → cancel request
                }
            }
        }
    })
}
