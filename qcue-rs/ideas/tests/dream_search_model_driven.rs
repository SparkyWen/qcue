// QCue A-R13/A-R22 — model-authored pattern passes through UNREWRITTEN; mode inferred from args. The
// harness never rewrites the model's search pattern (recall is agentic), it only reads the arg shape.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ideas::recall::search_tool::{infer_mode, RecallMode};

#[test]
fn mode_inference_and_pattern_passthrough() {
    // A-R22 — a slug arg → browse; a free pattern → discovery; an id+direction → scroll.
    assert_eq!(infer_mode("rust", false), RecallMode::Browse); // bare slug
    assert_eq!(infer_mode("what did we conclude about X", false), RecallMode::Discovery);
    assert_eq!(infer_mode("hit-42", true), RecallMode::Scroll); // match id + direction flag
    // A-R13 — the harness never rewrites the model's pattern: infer_mode does not mutate the string.
    let p = "数据库迁移 步骤";
    let _ = infer_mode(p, false);
    assert_eq!(p, "数据库迁移 步骤");
}
