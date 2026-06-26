//! Declarative provider profiles + hooks + registry + the wire-quirk table.
pub mod health;
pub mod hooks;
pub mod profile;
pub mod quirks;
pub mod registry;
pub mod resolve;
pub use resolve::effective_api_mode;
pub mod vendors {
    pub mod anthropic;
    pub mod deepseek;
    pub mod gemini;
    pub mod kimi;
    pub mod openai;
    pub mod openrouter;
    pub mod qwen;
}
