// QCue S1-R47 — re-export the llm-api scrub for router-side use on assembled assistant text.
// Distinct from `llm-api::scrub`'s streaming-time use: here it scrubs the COMPLETED assistant text
// (a forged `<tool_call>`/`</invoke>`-style wrapper smuggled into prose) before persistence/dispatch.
pub use llm_api::scrub::scrub_forged_wrappers;
