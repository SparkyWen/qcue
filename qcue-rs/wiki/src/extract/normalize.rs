// QCue S2-R17 — normalize irregular LLM batch JSON with a validity flag (PORT source-analyzer.ts:45-80).
use super::batch_merger::ExtractedItem;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validity {
    Valid,
    Empty,
    Unusable,
}

#[derive(Debug, Clone)]
pub struct NormalizedBatch {
    pub entities: Vec<ExtractedItem>,
    pub concepts: Vec<ExtractedItem>,
    pub validity: Validity,
}

#[allow(clippy::result_unit_err)]
fn coerce_items(v: Option<&Value>) -> Result<Vec<ExtractedItem>, ()> {
    match v {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::Array(arr)) => {
            let mut out = Vec::new();
            for it in arr {
                let name = it.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string();
                if name.is_empty() {
                    continue;
                }
                let aliases = it
                    .get("aliases")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                out.push(ExtractedItem { name, aliases });
            }
            Ok(out)
        }
        Some(_) => Err(()), // entities:true etc. → unusable
    }
}

pub fn normalize_batch_response(v: &Value) -> NormalizedBatch {
    let entities = coerce_items(v.get("entities"));
    let concepts = coerce_items(v.get("concepts"));
    match (entities, concepts) {
        (Err(_), _) | (_, Err(_)) => {
            NormalizedBatch { entities: vec![], concepts: vec![], validity: Validity::Unusable }
        }
        (Ok(e), Ok(c)) => {
            let validity = if e.is_empty() && c.is_empty() { Validity::Empty } else { Validity::Valid };
            NormalizedBatch { entities: e, concepts: c, validity }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use serde_json::json;
    #[test]
    fn validity_flag_handles_irregular_json() {
        // well-formed → Valid with items
        let r = normalize_batch_response(&json!({"entities":[{"name":"Rust","aliases":[]}],"concepts":[]}));
        assert_eq!(r.validity, Validity::Valid);
        assert_eq!(r.entities.len(), 1);
        // omitted arrays → Empty (usable, just nothing new)
        let r = normalize_batch_response(&json!({}));
        assert_eq!(r.validity, Validity::Empty);
        // entities:true (model hallucinated a bool) → Unusable
        let r = normalize_batch_response(&json!({"entities": true}));
        assert_eq!(r.validity, Validity::Unusable);
    }
}
