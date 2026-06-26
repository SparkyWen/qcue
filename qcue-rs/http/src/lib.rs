//! Provider-neutral HTTP transport: client builder, retry/backoff, SSE framing.
pub mod client;
pub mod retry;
pub mod sse;
pub mod ssrf;
