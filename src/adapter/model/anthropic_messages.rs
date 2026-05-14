use super::ModelAdapter;
use crate::adapter::types::{ToolDefinition, UnifiedResponse, ToolCallInfo};

/// Anthropic Messages API adapter.
///
/// Converts between OpenAI-format messages (used internally) and Anthropic-format
/// request/response bodies. All format differences are contained here — business
/// code never sees Anthropic-specific message shapes.
pub struct AnthropicMessagesAdapter {
    pub id: String,
}

impl AnthropicMessagesAdapter {
    pub fn new(id: impl Into<String>) -> Self {
        AnthropicMessagesAdapter { id: id.into() }
    }
}

impl ModelAdapter for AnthropicMessagesAdapter {
    fn adapter_id(&self) -> &str { &self.id }

    fn format_tools(&self, tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        tools.iter().map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            })
        }).collect()
    }

    fn build_request_body(
        &self,
        model: &str,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        _response_format: Option<&serde_json::Value>,
        extra_body: Option<&std::collections::HashMap<String, serde_json::Value>>,
    ) -> serde_json::Value {
        let mut system_text = String::new();
        let mut converted: Vec<serde_json::Value> = Vec::new();

        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            match role {
                "system" => {
                    system_text = format!("{}{}", system_text, msg.get("content").and_then(|c| c.as_str()).unwrap_or(""));
                }
                "user" => {
                    // If already Anthropic format (content is array of blocks), pass through
                    if msg.get("content").and_then(|c| c.as_array()).is_some() {
                        converted.push(msg.clone());
                    } else {
                        let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        converted.push(serde_json::json!({
                            "role": "user",
                            "content": [{"type": "text", "text": content}]
                        }));
                    }
                }
                "assistant" => {
                    // If already Anthropic format (content array), use as-is
                    if msg.get("content").and_then(|c| c.as_array()).is_some() {
                        converted.push(msg.clone());
                        continue;
                    }
                    // Convert from OpenAI format
                    let text = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    let mut blocks: Vec<serde_json::Value> = Vec::new();
                    if !text.is_empty() && text != "\n" {
                        blocks.push(serde_json::json!({"type": "text", "text": text}));
                    }
                    // Convert tool_calls to tool_use blocks
                    if let Some(tc_array) = msg.get("tool_calls").and_then(|t| t.as_array()) {
                        for tc in tc_array {
                            let tc_id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            let func = tc.get("function");
                            let name = func.and_then(|f| f.get("name")).and_then(|n| n.as_str()).unwrap_or("");
                            let args_str = func.and_then(|f| f.get("arguments")).and_then(|a| a.as_str()).unwrap_or("{}");
                            let input: serde_json::Value = serde_json::from_str(args_str).unwrap_or(serde_json::json!({}));
                            blocks.push(serde_json::json!({
                                "type": "tool_use",
                                "id": tc_id,
                                "name": name,
                                "input": input,
                            }));
                        }
                    }
                    if !blocks.is_empty() {
                        converted.push(serde_json::json!({"role": "assistant", "content": blocks}));
                    }
                }
                "tool" => {
                    let tc_id = msg.get("tool_call_id").and_then(|i| i.as_str()).unwrap_or("");
                    let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    converted.push(serde_json::json!({
                        "role": "user",
                        "content": [{"type": "tool_result", "tool_use_id": tc_id, "content": content}]
                    }));
                }
                _ => {}
            }
        }

        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": 8192,
            "messages": converted,
        });
        if !system_text.is_empty() {
            body["system"] = serde_json::json!(system_text);
        }
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools);
        }
        if let Some(eb) = extra_body {
            for (k, v) in eb {
                body[k] = v.clone();
            }
        }
        body
    }

    fn parse_response(&self, raw: &serde_json::Value) -> Result<UnifiedResponse, String> {
        let content = raw.get("content").and_then(|c| c.as_array())
            .ok_or_else(|| "Missing content array in Anthropic response".to_string())?;

        let mut tool_calls = Vec::new();
        let mut text_parts: Vec<String> = Vec::new();
        let mut reasoning_parts: Vec<String> = Vec::new();

        for block in content {
            match block.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "tool_use" => {
                    let id = block.get("id").and_then(|i| i.as_str()).map(String::from);
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                    let input = block.get("input").cloned().unwrap_or(serde_json::Value::Null);
                    if !name.is_empty() {
                        tool_calls.push(ToolCallInfo { id, name, arguments: input });
                    }
                }
                "text" => {
                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                        text_parts.push(t.to_string());
                    }
                }
                "thinking" => {
                    if let Some(t) = block.get("thinking").and_then(|t| t.as_str()) {
                        reasoning_parts.push(t.to_string());
                    }
                }
                _ => {}
            }
        }

        let text = if text_parts.is_empty() { None } else { Some(text_parts.join("\n")) };
        let reasoning = if reasoning_parts.is_empty() { None } else { Some(reasoning_parts.join("\n")) };

        Ok(UnifiedResponse { text, tool_calls, reasoning })
    }

    fn build_assistant_message(&self, raw: &serde_json::Value) -> Result<serde_json::Value, String> {
        let mut msg = raw.clone();
        if let Some(obj) = msg.as_object_mut() {
            obj.insert("role".to_string(), serde_json::json!("assistant"));
        }
        Ok(msg)
    }

    fn api_path(&self) -> &str { "/v1/messages" }

    fn build_assistant_with_tools(&self, tool_calls: &[crate::adapter::types::ToolCallInfo], _reasoning: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "role": "assistant",
            "content": tool_calls.iter().map(|tc| serde_json::json!({
                "type": "tool_use",
                "id": tc.id.clone().unwrap_or_default(),
                "name": tc.name,
                "input": tc.arguments,
            })).collect::<Vec<_>>()
        })
    }

    fn build_tool_result(&self, tool_call_id: &str, content: &str) -> serde_json::Value {
        serde_json::json!({
            "role": "user",
            "content": [{"type": "tool_result", "tool_use_id": tool_call_id, "content": content}]
        })
    }
}
