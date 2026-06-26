// QCue S2-R8/S2-R14/S2-R18 — single-call conversation extract; output language threaded into the prompt.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use uuid::Uuid;
use wiki::extract::analyzer::{AnalyzeMode, SourceAnalyzer};
use wiki::llm::StubWikiLlm;

#[tokio::test]
async fn conversation_mode_makes_single_extraction_call() {
    let llm = StubWikiLlm::scripted(vec![
        r#"{"source_title":"Rust notes","summary":"s","entities":[{"name":"Rust","aliases":[]}],"concepts":[],"contradictions":[],"related_pages":[],"key_points":[]}"#
            .into(),
    ]);
    let analyzer = SourceAnalyzer::new(&llm, "en");
    let (sa, calls) = analyzer
        .analyze(Uuid::now_v7(), "I learned Rust today", AnalyzeMode::Conversation)
        .await
        .unwrap();
    assert_eq!(calls, 1); // S2-R8: no batch loop for conversations
    assert_eq!(sa.entities.len(), 1);
    assert_eq!(llm.call_count(), 1);
}

#[tokio::test]
async fn output_language_is_threaded_into_prompt() {
    let llm = StubWikiLlm::recording();
    let analyzer = SourceAnalyzer::new(&llm, "zh");
    let _ = analyzer.analyze(Uuid::now_v7(), "text", AnalyzeMode::Conversation).await.unwrap();
    assert!(llm.last_system().contains("zh")); // S2-R18: language in the prompt
}
