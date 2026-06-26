#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R58..R61 — JSON robustness ladder; ranker never throws → [].
use router::structured::{parse_json_response, rank_or_empty, RepairLadder};

#[test]
fn test_strip_fences_and_think() {
    // S1-R58 — strip ```json fences and <think> blocks, brace-count to the object.
    let raw = "<think>reasoning</think>\nHere is the result:\n```json\n{\"a\": 1}\n```\nDone.";
    let v = parse_json_response(raw).unwrap();
    assert_eq!(v["a"], 1);
}

#[test]
fn test_prefill_brace_extraction() {
    // a bare object with leading prose still parses.
    let raw = "Sure! {\"b\": 2, \"c\": [1,2]}";
    let v = parse_json_response(raw).unwrap();
    assert_eq!(v["b"], 2);
}

#[test]
fn test_malformed_returns_err() {
    assert!(parse_json_response("not json at all").is_err());
}

#[tokio::test]
async fn test_one_repair_pass() {
    // S1-R59 — exactly one repair attempt; a still-bad second response errors (no loop).
    let mut ladder = RepairLadder::new();
    // first parse fails; the repair closure returns valid JSON exactly once.
    let result = ladder
        .parse_with_repair("{bad", |_malformed| async { Ok("{\"ok\":true}".to_string()) })
        .await;
    assert!(result.is_ok());
    assert_eq!(ladder.repair_attempts(), 1);

    let mut ladder2 = RepairLadder::new();
    let result2 = ladder2
        .parse_with_repair("{bad", |_| async { Ok("still bad".to_string()) })
        .await;
    assert!(result2.is_err());
    assert_eq!(ladder2.repair_attempts(), 1); // did NOT loop
}

#[tokio::test]
async fn test_ranker_fails_to_empty() {
    // S1-R61 — malformed/timeout/error each yield [] (never throws).
    let empty1 = rank_or_empty(async { Err::<String, String>("provider down".into()) }).await;
    assert!(empty1.is_empty());
    let empty2 = rank_or_empty(async { Ok::<String, String>("not json".into()) }).await;
    assert!(empty2.is_empty());
    let ok = rank_or_empty(async { Ok::<String, String>("{\"selected\":[\"a\",\"b\"]}".into()) }).await;
    assert_eq!(ok, vec!["a".to_string(), "b".to_string()]);
}
