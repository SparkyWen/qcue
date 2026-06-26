// QCue S1-R57..R61 — structured-output + JSON robustness ladder.
use serde_json::Value;
use std::future::Future;

/// S1-R58 — strip ```json fences + <think> blocks, then brace-count to the JSON object.
pub fn parse_json_response(raw: &str) -> Result<Value, String> {
    let mut s = raw.to_string();
    // strip <think>...</think>
    while let (Some(a), Some(b)) = (s.find("<think>"), s.find("</think>")) {
        if b > a {
            s.replace_range(a..b + "</think>".len(), "");
        } else {
            break;
        }
    }
    // strip ```json ... ``` fences (keep the inner)
    if let Some(start) = s.find("```") {
        let after = &s[start + 3..];
        let after = after.strip_prefix("json").unwrap_or(after);
        if let Some(end) = after.find("```") {
            s = after[..end].to_string();
        }
    }
    // brace-count to extract the first balanced {...}
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut start = None;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0
                    && let Some(st) = start
                {
                    return serde_json::from_str(&s[st..=i]).map_err(|e| e.to_string());
                }
            }
            _ => {}
        }
    }
    serde_json::from_str(s.trim()).map_err(|e| e.to_string())
}

/// S1-R59 — exactly one repair pass.
pub struct RepairLadder {
    attempts: u32,
}
impl RepairLadder {
    pub fn new() -> Self {
        Self { attempts: 0 }
    }
    pub fn repair_attempts(&self) -> u32 {
        self.attempts
    }
    /// Try to parse; on failure call `repair` ONCE; a second failure errors (no loop).
    pub async fn parse_with_repair<F, Fut>(&mut self, raw: &str, repair: F) -> Result<Value, String>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<String, String>>,
    {
        if let Ok(v) = parse_json_response(raw) {
            return Ok(v);
        }
        self.attempts += 1;
        let repaired = repair(raw.to_string()).await?;
        parse_json_response(&repaired)
    }
}
impl Default for RepairLadder {
    fn default() -> Self {
        Self::new()
    }
}

/// S1-R61 — the cheap-model ranker fails closed to [] on ANY parse/timeout/provider error.
pub async fn rank_or_empty<Fut>(fut: Fut) -> Vec<String>
where
    Fut: Future<Output = Result<String, String>>,
{
    match fut.await {
        Ok(body) => match parse_json_response(&body) {
            Ok(v) => v
                .get("selected")
                .and_then(|s| s.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    }
}
