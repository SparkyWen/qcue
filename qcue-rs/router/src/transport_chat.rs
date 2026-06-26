// QCue S1-R26,R51,R52,R57 — ChatCompletions transport. Strips storage-only fields; key-stable args.
use crate::transport::{ReqParams, Transport};
use llm_api::usage_norm::usage_from_chat;
use protocol::{
    ApiMode, FinishReason, Message, NormalizedResponse, Role, ToolCall, ToolDef, TransportError,
};
use providers::profile::{ProviderProfile, TempPolicy};
use serde_json::{Map, Value, json};

pub struct ChatCompletionsTransport;

fn role_str(r: Role) -> &'static str {
    match r {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

/// Re-serialize a JSON-string argument with sorted keys for byte stability (S1-R52).
pub(crate) fn sort_json_string(s: &str) -> String {
    match serde_json::from_str::<Value>(s) {
        Ok(v) => sorted_to_string(&v),
        Err(_) => s.to_string(),
    }
}
fn sorted_to_string(v: &Value) -> String {
    // serde_json::Value uses a BTreeMap for objects only with the "preserve_order" feature OFF,
    // which is the default — object keys serialize sorted. We rely on that default here.
    serde_json::to_string(v).unwrap_or_default()
}

/// The token-limit param name for a ChatCompletions model. OpenAI's gpt-5.x and o-series reject
/// `max_tokens` ("Use 'max_completion_tokens' instead", HTTP 400); every other ChatCompletions model
/// (gpt-4o, deepseek, kimi, qwen, openrouter, gemini-compat, …) uses the classic `max_tokens`.
fn token_limit_key(model: &str) -> &'static str {
    if model.starts_with("gpt-5")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
    {
        "max_completion_tokens"
    } else {
        "max_tokens"
    }
}

impl Transport for ChatCompletionsTransport {
    fn api_mode(&self) -> ApiMode {
        ApiMode::ChatCompletions
    }

    fn convert_messages(&self, msgs: &[Message], _model: &str) -> Value {
        // Build a COPY; strip storage-only fields (reasoning/finish_reason/provider_data thinking).
        let arr: Vec<Value> = msgs
            .iter()
            .map(|m| {
                let mut obj = Map::new();
                obj.insert("role".into(), json!(role_str(m.role)));
                if let Some(c) = &m.content {
                    obj.insert("content".into(), json!(c));
                }
                if let Some(tcs) = &m.tool_calls {
                    let calls: Vec<Value> = tcs
                        .iter()
                        .map(|tc: &ToolCall| {
                            json!({
                                "id": tc.id.clone().unwrap_or_default(),
                                "type": "function",
                                "function": { "name": tc.name, "arguments": sort_json_string(&tc.arguments) }
                            })
                        })
                        .collect();
                    obj.insert("tool_calls".into(), json!(calls));
                }
                if let Some(id) = &m.tool_call_id {
                    obj.insert("tool_call_id".into(), json!(id));
                }
                // NOTE: reasoning / finish_reason / provider_data(thinking) are intentionally NOT copied.
                Value::Object(obj)
            })
            .collect();
        json!(arr)
    }

    fn convert_tools(&self, tools: &[ToolDef]) -> Value {
        let arr: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": { "name": t.name, "description": t.description, "parameters": t.input_schema }
                })
            })
            .collect();
        json!(arr)
    }

    fn build_kwargs(
        &self,
        model: &str,
        msgs: &[Message],
        tools: Option<&[ToolDef]>,
        profile: &ProviderProfile,
        params: &ReqParams,
    ) -> Value {
        let mut body = Map::new();
        body.insert("model".into(), json!(model));
        body.insert("messages".into(), self.convert_messages(msgs, model));
        if let Some(t) = tools {
            body.insert("tools".into(), self.convert_tools(t));
            if !t.is_empty() {
                // S1/F-14 — advertise the tools as usable. This was never set, so some ChatCompletions
                // models (DeepSeek especially) tended to answer in prose instead of calling the tool.
                // `auto` (the spec default, made explicit) still lets recall answer without a tool call.
                body.insert("tool_choice".into(), json!("auto"));
            }
        }
        match profile.fixed_temperature {
            TempPolicy::Omit => {} // no temperature key (Kimi)
            TempPolicy::Fixed(f) => {
                body.insert("temperature".into(), json!(f));
            }
            TempPolicy::Inherit => {
                if let Some(t) = params.temperature {
                    body.insert("temperature".into(), json!(t));
                }
            }
        }
        // max_tokens precedence: explicit param > provider hook (get_max_tokens) > profile default (F-2).
        if let Some(mt) = params
            .max_tokens
            .or_else(|| profile.hooks.get_max_tokens(model))
            .or(profile.default_max_tokens)
        {
            body.insert(token_limit_key(model).into(), json!(mt));
        }
        if params.stream {
            body.insert("stream".into(), json!(true));
            body.insert("stream_options".into(), json!({"include_usage": true}));
        }
        if let Some(rf) = &params.response_format {
            body.insert("response_format".into(), rf.clone());
        }
        // F-1 — provider-native web search. On the chat/completions wire it is a body param (NOT a function
        // tool); only search-capable models (e.g. gpt-4o-search-preview) honor it — others ignore/reject it.
        if params
            .server_tools
            .iter()
            .any(|s| matches!(s, crate::transport::ServerTool::WebSearch { .. }))
        {
            body.insert("web_search_options".into(), json!({}));
        }
        Value::Object(body)
    }

    fn normalize_response(&self, raw: &Value) -> Result<NormalizedResponse, TransportError> {
        let choice = raw
            .get("choices")
            .and_then(|c| c.get(0))
            .ok_or_else(|| TransportError::MissingField("choices[0]".into()))?;
        let msg = choice
            .get("message")
            .ok_or_else(|| TransportError::MissingField("message".into()))?;
        let content = msg.get("content").and_then(|c| c.as_str()).map(String::from);
        let reasoning = msg
            .get("reasoning_content")
            .or_else(|| msg.get("reasoning"))
            .and_then(|c| c.as_str())
            .map(String::from);
        let tool_calls = msg.get("tool_calls").and_then(|t| t.as_array()).map(|arr| {
            arr.iter()
                .map(|tc| ToolCall {
                    id: tc.get("id").and_then(|x| x.as_str()).map(String::from),
                    name: tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    arguments: tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("{}")
                        .into(),
                    provider_data: None,
                })
                .collect()
        });
        let finish = match choice
            .get("finish_reason")
            .and_then(|x| x.as_str())
            .unwrap_or("stop")
        {
            "tool_calls" => FinishReason::ToolCalls,
            "length" => FinishReason::Length,
            "content_filter" => FinishReason::ContentFilter,
            _ => FinishReason::Stop,
        };
        let usage = raw.get("usage").and_then(usage_from_chat);
        let provider_data = reasoning.as_ref().map(|r| json!({ "reasoning_content": r }));
        Ok(NormalizedResponse {
            content,
            tool_calls,
            finish_reason: finish,
            reasoning,
            usage,
            provider_data,
        })
    }
}
