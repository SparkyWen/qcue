// QCue S1-R26,R57,R66,R77 — Anthropic transport. Reconstructs blocks + replays opaque thinking signatures.
use crate::transport::{ReqParams, ServerTool, Transport};
use llm_api::usage_norm::usage_from_anthropic;
use protocol::{ApiMode, FinishReason, Message, NormalizedResponse, Role, ToolDef, TransportError};
use providers::profile::ProviderProfile;
use serde_json::{Map, Value, json};

pub struct AnthropicTransport;

/// Append a tool object to the request body's `tools` array (creating it if absent) — merges rather than
/// clobbers, so server tools (web_search) and the structured-output `emit` tool coexist with client tools.
fn push_tool(body: &mut Map<String, Value>, tool: Value) {
    match body.get_mut("tools").and_then(|t| t.as_array_mut()) {
        Some(arr) => arr.push(tool),
        None => {
            body.insert("tools".into(), json!([tool]));
        }
    }
}

impl Transport for AnthropicTransport {
    fn api_mode(&self) -> ApiMode {
        ApiMode::AnthropicMessages
    }

    fn convert_messages(&self, msgs: &[Message], _model: &str) -> Value {
        // Anthropic: system is hoisted out; thinking signature replayed byte-unchanged from provider_data.
        // A Role::Tool result becomes a `tool_result` content block (carrying the `tool_use_id`) inside a
        // USER message — Anthropic REQUIRES a tool_result IMMEDIATELY after a tool_use and rejects a plain
        // text user message there with a 400 ("`tool_use` ids ... without `tool_result` blocks"). This is
        // THE fix that makes multi-step tool use (recall/Dream) work on Anthropic: without it the model
        // could call a tool once and the loop's next request 400'd. Consecutive tool results (a parallel
        // tool_use batch) coalesce into ONE user message of tool_result blocks (two user messages in a row
        // would itself be a role-alternation 400).
        let non_system: Vec<&Message> = msgs.iter().filter(|m| m.role != Role::System).collect();
        let mut out: Vec<Value> = Vec::new();
        let mut i = 0;
        while i < non_system.len() {
            if non_system[i].role == Role::Tool {
                let mut blocks: Vec<Value> = Vec::new();
                while i < non_system.len() && non_system[i].role == Role::Tool {
                    let tm = non_system[i];
                    blocks.push(json!({
                        "type": "tool_result",
                        "tool_use_id": tm.tool_call_id.clone().unwrap_or_default(),
                        "content": tm.content.clone().unwrap_or_default(),
                    }));
                    i += 1;
                }
                out.push(json!({ "role": "user", "content": blocks }));
                continue;
            }
            let m = non_system[i];
            let role = if m.role == Role::Assistant { "assistant" } else { "user" };
            let mut blocks: Vec<Value> = Vec::new();
            // replay opaque thinking block FIRST (S1-R77), preserving order before text/tool_use.
            if let Some(thinking) = m.provider_data.as_ref().and_then(|pd| pd.get("thinking")) {
                blocks.push(json!({ "type": "thinking",
                    "thinking": thinking.get("thinking").and_then(|x| x.as_str()).unwrap_or(""),
                    "signature": thinking.get("signature").and_then(|x| x.as_str()).unwrap_or("") }));
            }
            if let Some(c) = &m.content
                && !c.is_empty()
            {
                blocks.push(json!({"type":"text","text":c}));
            }
            if let Some(tcs) = &m.tool_calls {
                for tc in tcs {
                    let input: Value = serde_json::from_str(&tc.arguments).unwrap_or(json!({}));
                    blocks.push(json!({"type":"tool_use","id":tc.id.clone().unwrap_or_default(),"name":tc.name,"input":input}));
                }
            }
            // Anthropic rejects an empty content array — guarantee at least one block.
            if blocks.is_empty() {
                blocks.push(json!({"type":"text","text":"(empty)"}));
            }
            out.push(json!({ "role": role, "content": blocks }));
            i += 1;
        }
        json!(out)
    }

    fn convert_tools(&self, tools: &[ToolDef]) -> Value {
        let arr: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name, "description": t.description, "input_schema": t.input_schema })
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
        // hoist system blocks (S1-R66 cache_control applied in prompt_cache; here just the text).
        let system: String = msgs
            .iter()
            .filter(|m| m.role == Role::System)
            .filter_map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        if !system.is_empty() {
            body.insert("system".into(), json!(system));
        }
        body.insert("messages".into(), self.convert_messages(msgs, model));
        if let Some(t) = tools {
            body.insert("tools".into(), self.convert_tools(t));
        }
        body.insert(
            "max_tokens".into(),
            // explicit param > provider hook (get_max_tokens) > profile default > 4096 (F-2).
            json!(params
                .max_tokens
                .or_else(|| profile.hooks.get_max_tokens(model))
                .or(profile.default_max_tokens)
                .unwrap_or(4096)),
        );
        if let Some(temp) = params.temperature {
            body.insert("temperature".into(), json!(temp));
        }
        if params.stream {
            body.insert("stream".into(), json!(true));
        }
        // F-1 — provider-native server tools (Anthropic runs the search; results return inline as
        // server_tool_use / web_search_tool_result blocks, never a client round-trip). Merged into `tools`.
        for st in &params.server_tools {
            match st {
                ServerTool::WebSearch { max_uses } => {
                    let mut t = Map::new();
                    t.insert("type".into(), json!("web_search_20260209"));
                    t.insert("name".into(), json!("web_search"));
                    if let Some(n) = max_uses {
                        t.insert("max_uses".into(), json!(n));
                    }
                    push_tool(&mut body, Value::Object(t));
                }
            }
        }
        // structured output via tool-forcing (S1-R57): an `emit` tool whose input schema is the target.
        // F-15 — MERGE it into any already-advertised tools (recall_search/propose_*) instead of clobbering
        // them, and force `emit` ONLY when it is the sole tool (forcing it alongside real tools would
        // block the model from ever calling them).
        if let Some(rf) = &params.response_format
            && let Some(schema) = rf.get("json_schema").and_then(|j| j.get("schema"))
        {
            let emit = json!({"name":"emit","description":"emit the JSON","input_schema":schema});
            push_tool(&mut body, emit);
            let only_emit = body
                .get("tools")
                .and_then(|t| t.as_array())
                .map(|a| a.len() == 1)
                .unwrap_or(true);
            if only_emit {
                body.insert("tool_choice".into(), json!({"type":"tool","name":"emit"}));
            }
        }
        Value::Object(body)
    }

    fn normalize_response(&self, raw: &Value) -> Result<NormalizedResponse, TransportError> {
        let content_blocks = raw
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| TransportError::MissingField("content".into()))?;
        let mut text = String::new();
        let mut reasoning = None;
        let mut tool_calls = Vec::new();
        for b in content_blocks {
            match b.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "text" => text.push_str(b.get("text").and_then(|t| t.as_str()).unwrap_or("")),
                "thinking" => {
                    reasoning = b.get("thinking").and_then(|t| t.as_str()).map(String::from)
                }
                "tool_use" => tool_calls.push(protocol::ToolCall {
                    id: b.get("id").and_then(|x| x.as_str()).map(String::from),
                    name: b.get("name").and_then(|x| x.as_str()).unwrap_or("").into(),
                    arguments: b
                        .get("input")
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "{}".into()),
                    provider_data: None,
                }),
                _ => {}
            }
        }
        let finish = match raw
            .get("stop_reason")
            .and_then(|x| x.as_str())
            .unwrap_or("end_turn")
        {
            "tool_use" => FinishReason::ToolCalls,
            "max_tokens" => FinishReason::Length,
            "refusal" => FinishReason::ContentFilter,
            _ => FinishReason::Stop,
        };
        let usage = raw.get("usage").and_then(usage_from_anthropic);
        Ok(NormalizedResponse {
            content: if text.is_empty() { None } else { Some(text) },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            finish_reason: finish,
            reasoning,
            usage,
            provider_data: raw
                .get("content")
                .cloned()
                .map(|c| json!({ "anthropic_content_blocks": c })),
        })
    }
}
