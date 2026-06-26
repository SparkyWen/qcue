// QCue S1-R23/R24 — byte stream → SSE frames. 10MB buffer cap; multi-line data accumulation.
use bytes::Bytes;
use futures_util::Stream;
use std::pin::Pin;

pub const MAX_SSE_BUFFER: usize = 10 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SseFrame {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Debug)]
pub enum SseError {
    Overflow,
    Io(String),
}
impl std::fmt::Display for SseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SseError::Overflow => write!(f, "sse buffer overflow"),
            SseError::Io(e) => write!(f, "sse io: {e}"),
        }
    }
}
impl std::error::Error for SseError {}

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Parse a raw byte stream into SSE frames split on the blank-line (`\n\n`) boundary.
pub fn sse_frames(mut bytes: ByteStream) -> impl Stream<Item = Result<SseFrame, SseError>> + Send {
    use futures_util::StreamExt;
    async_stream::stream! {
        let mut buf = String::new();
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk { Ok(c) => c, Err(e) => { yield Err(SseError::Io(e.to_string())); return; } };
            buf.push_str(&String::from_utf8_lossy(&chunk));
            if buf.len() > MAX_SSE_BUFFER { yield Err(SseError::Overflow); return; }
            while let Some(idx) = buf.find("\n\n") {
                let raw = buf[..idx].to_string();
                buf.drain(..idx + 2);
                if let Some(frame) = parse_frame(&raw) { yield Ok(frame); }
            }
        }
        // Plan note: collapsed to a let-chain to satisfy clippy::collapsible_if (edition-2024);
        // behavior identical to the plan's nested form.
        if !buf.trim().is_empty()
            && let Some(frame) = parse_frame(buf.trim())
        {
            yield Ok(frame);
        }
    }
}

fn parse_frame(raw: &str) -> Option<SseFrame> {
    let mut event = None;
    let mut data_lines = Vec::new();
    for line in raw.lines() {
        if let Some(v) = line.strip_prefix("data:") {
            data_lines.push(v.trim_start().to_string());
        } else if let Some(v) = line.strip_prefix("event:") {
            event = Some(v.trim().to_string());
        }
        // unknown keys (id:, retry:, comments) are skipped (forward-compat)
    }
    if data_lines.is_empty() && event.is_none() {
        return None;
    }
    Some(SseFrame { event, data: data_lines.join("\n") })
}
