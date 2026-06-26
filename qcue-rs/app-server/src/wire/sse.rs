//! QCue S3-R37/R39 — SSE fan-out (a tokio `broadcast` channel → an axum `Sse` response) + a 15s
//! keep-alive heartbeat so idle proxies don't drop the stream. The per-event `id:` carries the
//! envelope `seq` so a reconnecting `EventSource` can resume with `Last-Event-ID` / `?since_seq=`
//! against the replay ring (Task 22).
//!
//! Auth for these GET streams is the dual extractor's `?token=` allowlist (`EventSource` can't set an
//! `Authorization` header — pitfall #15); the route mounts `TenantCtx`, which enforces it.
use app_server_protocol::RuntimeEventEnvelope;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

/// The keep-alive heartbeat interval — short enough that proxies (and the Flutter client) never treat
/// an idle Dream/recall stream as dead.
pub const HEARTBEAT_SECS: u64 = 15;

/// Build an SSE response from a per-Thread/per-job broadcast receiver, with the 15s heartbeat (S3-R39).
/// One envelope → an SSE `Event` carrying the `seq` as the `id:` (so a reconnecting client can resume).
fn to_event(env: &RuntimeEventEnvelope) -> Event {
    Event::default().id(env.seq.to_string()).json_data(env).unwrap_or_else(|_| Event::default())
}

/// A lagged receiver (the slow consumer fell behind the broadcast buffer) drops the laggy frames; the
/// client reconnects with `since_seq` and the replay ring backfills via [`sse_with_backfill`] (Task 22).
pub fn sse_from_broadcast(
    rx: broadcast::Receiver<RuntimeEventEnvelope>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    sse_with_backfill(Vec::new(), rx)
}

/// Build an SSE response that FIRST replays a `backfill` tail (the events a reconnecting client missed,
/// from the replay ring) and THEN streams the live broadcast — de-duplicating any live event whose
/// `seq` was already replayed. This is what makes `?since_seq=` / `Last-Event-ID` reconnect actually
/// resume instead of silently dropping the missed tail (S3-R37/R38, Task 22).
pub fn sse_with_backfill(
    backfill: Vec<RuntimeEventEnvelope>,
    rx: broadcast::Receiver<RuntimeEventEnvelope>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let last_seq = backfill.iter().map(|e| e.seq).max();
    let head =
        tokio_stream::iter(backfill.iter().map(|e| Ok::<Event, Infallible>(to_event(e))).collect::<Vec<_>>());
    let live = BroadcastStream::new(rx).filter_map(move |res| match res {
        // skip any live frame already covered by the backfill (seq <= the last replayed seq).
        Ok(env) => match last_seq {
            Some(ls) if env.seq <= ls => None,
            _ => Some(Ok(to_event(&env))),
        },
        Err(_) => None, // lagged → client reconnects with since_seq (replay ring backfills)
    });
    Sse::new(head.chain(live))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(HEARTBEAT_SECS)).text("keep-alive"))
}
