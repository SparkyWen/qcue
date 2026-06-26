//! Typed wire + SSE→StreamEvent parsers. Raw provider JSON never escapes this crate.
pub mod anthropic_sse;
pub mod chat_sse;
pub mod responses_sse;
pub mod scrub;
pub mod usage_norm;
pub mod watchdog;
