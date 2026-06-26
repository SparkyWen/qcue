// QCue D19 / RESP-R3..R6 — OpenAI **/v1/responses** transport. Carries gpt-5.x, where reasoning effort +
// function tools + structured output coexist (chat/completions 400s the combo — see
// docs/postmortems/2026-06-19-gpt5x-reasoning-effort-breaks-tools.md). Mirrors ChatCompletionsTransport
// method-for-method but renders the OpenAI-shaped transcript to the Responses wire (input/instructions/flat
// tools/reasoning.effort/function_call_output) and parses `output[]` back to the same NormalizedResponse.
use crate::transport::{ReqParams, ServerTool, Transport};
use crate::transport_chat::sort_json_string;
use llm_api::usage_norm::usage_from_chat;
use protocol::{
    ApiMode, FinishReason, Message, NormalizedResponse, Role, ToolCall, ToolDef, TransportError,
};
use providers::hooks::Effort;
use providers::profile::ProviderProfile;
use serde_json::{Map, Value, json};

pub struct ResponsesTransport;

/// Map QCue's 6 effort tiers onto the Responses `reasoning.effort` tokens. Only gpt-5.x models reach this
/// transport, and that generation supports low < medium < high < xhigh — NOT "minimal" (that was the
/// original gpt-5 / chat-completions tier). So clamp Minimal UP to "low" (else gpt-5.5 400s the "Instant"
/// Intelligence level); QCue's top two tiers (XHigh/Max) clamp to "xhigh".
fn responses_effort_str(effort: Effort) -> &'static str {
    match effort {
        Effort::Minimal | Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::XHigh | Effort::Max => "xhigh",
    }
}

impl Transport for ResponsesTransport {
    fn api_mode(&self) -> ApiMode {
        ApiMode::Responses
    }

    /// Render the transcript to the Responses `input` array (typed items, NOT chat `messages`):
    ///   - System → omitted (hoisted to top-level `instructions` in build_kwargs).
    ///   - User → `{role:"user", content:<string>}`.
    ///   - Assistant → (a) any stashed reasoning items echoed UNTOUCHED first (RESP-R9 — the Responses
    ///     analogue of S1-R77 thinking replay; reasoning models want reasoning→function_call→output order);
    ///     (b) `{role:"assistant", content:[{type:"output_text", text}]}` when there's text; (c) one
    ///     `{type:"function_call", call_id, name, arguments}` per tool_call.
    ///   - Tool (result) → `{type:"function_call_output", call_id, output}` — call_id MUST equal the model's
    ///     function_call.call_id (we store that in ToolCall.id / Message.tool_call_id); echoing the item id
    ///     (`fc_…`) 400s.
    fn convert_messages(&self, msgs: &[Message], _model: &str) -> Value {
        let mut out: Vec<Value> = Vec::new();
        for m in msgs {
            match m.role {
                Role::System => {} // hoisted to `instructions`
                Role::User => {
                    out.push(json!({ "role": "user", "content": m.content.clone().unwrap_or_default() }));
                }
                Role::Assistant => {
                    // (a) echo stashed reasoning items verbatim, in order, before the rest.
                    if let Some(items) = m
                        .provider_data
                        .as_ref()
                        .and_then(|pd| pd.get("responses_reasoning_items"))
                        .and_then(|x| x.as_array())
                    {
                        for it in items {
                            out.push(it.clone());
                        }
                    }
                    // (b) assistant text as an output_text content part.
                    if let Some(c) = &m.content
                        && !c.is_empty()
                    {
                        out.push(json!({
                            "role": "assistant",
                            "content": [{ "type": "output_text", "text": c }]
                        }));
                    }
                    // (c) one function_call item per tool call (call_id is the linkage id).
                    if let Some(tcs) = &m.tool_calls {
                        for tc in tcs {
                            out.push(json!({
                                "type": "function_call",
                                "call_id": tc.id.clone().unwrap_or_default(),
                                "name": tc.name,
                                "arguments": sort_json_string(&tc.arguments),
                            }));
                        }
                    }
                }
                Role::Tool => {
                    out.push(json!({
                        "type": "function_call_output",
                        "call_id": m.tool_call_id.clone().unwrap_or_default(),
                        "output": m.content.clone().unwrap_or_default(),
                    }));
                }
            }
        }
        json!(out)
    }

    /// Responses function tools are FLAT (`{type:"function", name, description, parameters}`) — no nested
    /// `function:{}` wrapper like chat/completions.
    fn convert_tools(&self, tools: &[ToolDef]) -> Value {
        let arr: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
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

        // System blocks hoisted to top-level `instructions` (NOT an input item).
        let system: String = msgs
            .iter()
            .filter(|m| m.role == Role::System)
            .filter_map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        if !system.is_empty() {
            body.insert("instructions".into(), json!(system));
        }
        body.insert("input".into(), self.convert_messages(msgs, model));

        // FLAT function tools + tool_choice; plus any native server tools (web_search) merged in.
        let mut tool_arr: Vec<Value> =
            tools.map(|t| self.convert_tools(t)).and_then(|v| v.as_array().cloned()).unwrap_or_default();
        for st in &params.server_tools {
            match st {
                ServerTool::WebSearch { .. } => tool_arr.push(json!({ "type": "web_search" })),
            }
        }
        if !tool_arr.is_empty() {
            let has_fn = tool_arr.iter().any(|t| t.get("type").and_then(|x| x.as_str()) == Some("function"));
            body.insert("tools".into(), json!(tool_arr));
            if has_fn {
                body.insert("tool_choice".into(), json!("auto"));
            }
        }

        // reasoning effort — the WIN: a native object that coexists with tools (no 400). Written here, NOT
        // via the chat `apply_reasoning_effort` hook (which speaks the chat `reasoning_effort` key).
        if let Some(effort) = params.reasoning.as_ref().and_then(|r| r.effort) {
            body.insert("reasoning".into(), json!({ "effort": responses_effort_str(effort) }));
        }

        // ONE token-limit key for all Responses models (includes reasoning tokens).
        if let Some(mt) = params
            .max_tokens
            .or_else(|| profile.hooks.get_max_tokens(model))
            .or(profile.default_max_tokens)
        {
            body.insert("max_output_tokens".into(), json!(mt));
        }

        // Structured output → `text.format` (NOT chat's top-level response_format). The canonical wrapper
        // QCue builds is {type:json_schema, json_schema:{name, schema}} (router_llm.rs); unwrap it.
        if let Some(js) = params.response_format.as_ref().and_then(|rf| rf.get("json_schema")) {
            let name = js.get("name").and_then(|n| n.as_str()).unwrap_or("output");
            if let Some(schema) = js.get("schema") {
                body.insert(
                    "text".into(),
                    json!({ "format": { "type": "json_schema", "name": name, "schema": schema, "strict": true } }),
                );
            }
        }

        // Stateless reasoning round-trip: don't persist server-side, but DO return encrypted reasoning so the
        // next tool iteration can echo it (RESP-R9). Temperature is intentionally omitted — gpt-5.x reasoning
        // models reject it on /v1/responses.
        body.insert("store".into(), json!(false));
        body.insert("include".into(), json!(["reasoning.encrypted_content"]));

        if params.stream {
            body.insert("stream".into(), json!(true));
        }
        Value::Object(body)
    }

    fn normalize_response(&self, raw: &Value) -> Result<NormalizedResponse, TransportError> {
        let output = raw
            .get("output")
            .and_then(|o| o.as_array())
            .ok_or_else(|| TransportError::MissingField("output".into()))?;

        let mut text = String::new();
        let mut reasoning: Option<String> = None;
        let mut reasoning_items: Vec<Value> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for item in output {
            match item.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "message" => {
                    if let Some(parts) = item.get("content").and_then(|c| c.as_array()) {
                        for p in parts {
                            if p.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                text.push_str(p.get("text").and_then(|t| t.as_str()).unwrap_or(""));
                            }
                        }
                    }
                }
                "reasoning" => {
                    // Stash the WHOLE item for echo (RESP-R9); also surface its summary text as `reasoning`.
                    reasoning_items.push(item.clone());
                    if let Some(sum) = item.get("summary").and_then(|s| s.as_array()) {
                        let joined: String = sum
                            .iter()
                            .filter_map(|s| s.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("");
                        if !joined.is_empty() {
                            reasoning = Some(joined);
                        }
                    }
                }
                "function_call" => {
                    tool_calls.push(ToolCall {
                        // Store the call_id (`call_…`), NOT the item id (`fc_…`) — it's what we echo as
                        // function_call_output.call_id next turn; the wrong id → 400.
                        id: item.get("call_id").and_then(|x| x.as_str()).map(String::from),
                        name: item.get("name").and_then(|x| x.as_str()).unwrap_or("").into(),
                        arguments: item.get("arguments").and_then(|x| x.as_str()).unwrap_or("{}").into(),
                        provider_data: None,
                    });
                }
                _ => {} // web_search_call, file_search_call, … affect text via annotations, not tool_calls
            }
        }

        // finish: a function_call item is the STRUCTURAL tool-call signal (Responses has no finish_reason).
        let finish = if !tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            match raw.get("status").and_then(|s| s.as_str()).unwrap_or("completed") {
                "incomplete" => match raw
                    .get("incomplete_details")
                    .and_then(|d| d.get("reason"))
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                {
                    "max_output_tokens" => FinishReason::Length,
                    "content_filter" => FinishReason::ContentFilter,
                    _ => FinishReason::Stop,
                },
                _ => FinishReason::Stop,
            }
        };

        let usage = raw.get("usage").and_then(usage_from_chat);
        let provider_data = (!reasoning_items.is_empty() || raw.get("id").is_some()).then(|| {
            json!({ "responses_reasoning_items": reasoning_items, "response_id": raw.get("id") })
        });

        Ok(NormalizedResponse {
            content: if text.is_empty() { None } else { Some(text) },
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            finish_reason: finish,
            reasoning,
            usage,
            provider_data,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::transport::ReqParams;
    use providers::hooks::{Effort, ReasoningConfig};
    use providers::registry::register_all;
    use serde_json::json;

    fn msg(role: Role, content: Option<&str>) -> Message {
        Message {
            role,
            content: content.map(String::from),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            finish_reason: None,
            reasoning: None,
            provider_data: None,
            active: true,
            is_untrusted: false,
        }
    }
    fn tool() -> ToolDef {
        ToolDef {
            name: "web_search".into(),
            description: "search".into(),
            input_schema: json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
        }
    }

    #[test]
    fn build_kwargs_renders_responses_request() {
        let reg = register_all();
        let profile = reg.get("openai").unwrap();
        let params = ReqParams {
            max_tokens: Some(2048),
            reasoning: Some(ReasoningConfig { effort: Some(Effort::High) }),
            ..Default::default()
        };
        let body = ResponsesTransport.build_kwargs(
            "gpt-5.5",
            &[msg(Role::System, Some("SYS")), msg(Role::User, Some("hi"))],
            Some(&[tool()]),
            profile,
            &params,
        );
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["name"], "web_search");
        assert!(body["tools"][0].get("function").is_none(), "Responses tools are FLAT: {body}");
        assert_eq!(body["instructions"], "SYS");
        assert!(
            body["input"].as_array().unwrap().iter().all(|i| i["role"] != "system"),
            "no system input item: {body}"
        );
        assert_eq!(body["reasoning"]["effort"], "high");
        assert!(body.get("reasoning_effort").is_none(), "use the Responses reasoning object, not the chat key");
        assert_eq!(body["max_output_tokens"], 2048);
        assert!(body.get("max_tokens").is_none() && body.get("max_completion_tokens").is_none());
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["store"], false);
        assert_eq!(body["include"][0], "reasoning.encrypted_content");
        assert!(body.get("temperature").is_none(), "reasoning models reject temperature");
    }

    #[test]
    fn convert_messages_threads_tool_calls_and_results_by_call_id() {
        let mut assistant = msg(Role::Assistant, None);
        assistant.tool_calls = Some(vec![ToolCall {
            id: Some("call_xyz".into()),
            name: "web_search".into(),
            arguments: "{\"query\":\"rust\"}".into(),
            provider_data: None,
        }]);
        let mut toolres = msg(Role::Tool, Some("RESULT"));
        toolres.tool_call_id = Some("call_xyz".into());
        toolres.tool_name = Some("web_search".into());

        let input =
            ResponsesTransport.convert_messages(&[msg(Role::User, Some("hi")), assistant, toolres], "gpt-5.5");
        let arr = input.as_array().unwrap();
        let fc = arr.iter().find(|i| i["type"] == "function_call").unwrap();
        assert_eq!(fc["call_id"], "call_xyz");
        assert_eq!(fc["name"], "web_search");
        let out = arr.iter().find(|i| i["type"] == "function_call_output").unwrap();
        assert_eq!(out["call_id"], "call_xyz", "result MUST pair by call_id, not fc_ id");
        assert_eq!(out["output"], "RESULT");
    }

    #[test]
    fn convert_messages_echoes_stashed_reasoning_item_before_function_call() {
        // RESP-R9 — a prior assistant turn whose provider_data stashed a reasoning item must re-emit it
        // (untouched) BEFORE its function_call, so a reasoning model sees reasoning→function_call→output.
        let mut assistant = msg(Role::Assistant, None);
        assistant.provider_data = Some(json!({
            "responses_reasoning_items": [{ "type": "reasoning", "id": "rs_1", "summary": [] }]
        }));
        assistant.tool_calls = Some(vec![ToolCall {
            id: Some("call_1".into()),
            name: "web_search".into(),
            arguments: "{}".into(),
            provider_data: None,
        }]);
        let input = ResponsesTransport.convert_messages(&[assistant], "gpt-5.5");
        let arr = input.as_array().unwrap();
        let r_pos = arr.iter().position(|i| i["type"] == "reasoning").unwrap();
        let f_pos = arr.iter().position(|i| i["type"] == "function_call").unwrap();
        assert!(r_pos < f_pos, "reasoning item must precede its function_call: {input}");
    }

    #[test]
    fn structured_output_renders_text_format() {
        let reg = register_all();
        let profile = reg.get("openai").unwrap();
        let params = ReqParams {
            response_format: Some(json!({"type":"json_schema","json_schema":{"name":"Out","schema":{"type":"object"}}})),
            ..Default::default()
        };
        let body = ResponsesTransport.build_kwargs("gpt-5.5", &[msg(Role::User, Some("hi"))], None, profile, &params);
        assert_eq!(body["text"]["format"]["type"], "json_schema");
        assert_eq!(body["text"]["format"]["name"], "Out");
        assert_eq!(body["text"]["format"]["schema"]["type"], "object");
        assert!(body.get("response_format").is_none(), "Responses uses text.format, not response_format");
    }

    #[test]
    fn normalize_parses_output_items_and_usage() {
        let raw = json!({
            "id":"resp_1","status":"completed",
            "output":[
                {"type":"reasoning","id":"rs_1","summary":[{"type":"summary_text","text":"thinking"}]},
                {"type":"function_call","id":"fc_1","call_id":"call_xyz","name":"web_search","arguments":"{\"query\":\"rust\"}","status":"completed"}
            ],
            "usage":{"input_tokens":100,"output_tokens":50,"input_tokens_details":{"cached_tokens":20},"output_tokens_details":{"reasoning_tokens":30}}
        });
        let nr = ResponsesTransport.normalize_response(&raw).unwrap();
        assert_eq!(nr.finish_reason, FinishReason::ToolCalls, "a function_call item ⇒ ToolCalls");
        let tc = &nr.tool_calls.unwrap()[0];
        assert_eq!(tc.id.as_deref(), Some("call_xyz"), "store call_id, NOT fc_id");
        assert_eq!(tc.name, "web_search");
        assert_eq!(tc.arguments, "{\"query\":\"rust\"}");
        let u = nr.usage.unwrap();
        assert_eq!(u.input, 100);
        assert_eq!(u.output, 50);
        assert_eq!(u.cache_read, 20);
        assert_eq!(u.reasoning, 30, "reasoning nests under output_tokens_details");
        // the reasoning item is stashed for the next turn's echo.
        assert_eq!(nr.provider_data.unwrap()["responses_reasoning_items"][0]["id"], "rs_1");
    }

    #[test]
    fn normalize_message_text_and_stop() {
        let raw = json!({"id":"r","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}],"usage":{"input_tokens":1,"output_tokens":1}});
        let nr = ResponsesTransport.normalize_response(&raw).unwrap();
        assert_eq!(nr.content.as_deref(), Some("hello"));
        assert_eq!(nr.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn transport_for_routes_responses() {
        // RESP-R1 — transport_for stays the single api_mode switch; the Responses arm is reachable.
        assert_eq!(crate::transport::transport_for(ApiMode::Responses).api_mode(), ApiMode::Responses);
    }

    #[test]
    fn normalize_incomplete_maps_to_length() {
        let raw = json!({"id":"r","status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[],"usage":{"input_tokens":1,"output_tokens":1}});
        let nr = ResponsesTransport.normalize_response(&raw).unwrap();
        assert_eq!(nr.finish_reason, FinishReason::Length);
    }
}
