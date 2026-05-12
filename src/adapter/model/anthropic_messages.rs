use super::ModelAdapter;
use crate::adapter::types::{ToolDefinition, UnifiedResponse, ToolCallInfo};

/// Anthropic Messages API adapter.
///
/// Supports any model that implements the Anthropic `/v1/messages` protocol.
/// Tool format differs from OpenAI Chat:
/// - Tools use `input_schema` instead of `parameters`
/// - No `type: "function"` wrapper
/// - Response uses content blocks (`tool_use`, `text`, `thinking`) instead of
///   `choices[0].message.tool_calls`
///
/// Currently a placeholder — the existing infrastructure uses OpenAI Chat
/// format. This adapter is ready for when Anthropic-compatible endpoints
/// (including MiniMax's `/anthropic` endpoint) are needed.
pub struct AnthropicMessagesAdapter {
    pub id: String,
}

impl AnthropicMessagesAdapter {
    pub fn new(id: impl Into<String>) -> Self {
        AnthropicMessagesAdapter { id: id.into() }
    }
}

impl ModelAdapter for AnthropicMessagesAdapter {
    fn adapter_id(&self) -> &str {
        &self.id
    }

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
        response_format: Option<&serde_json::Value>,
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": 8192,
            "messages": messages,
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools);
        }
        if let Some(rf) = response_format {
            body["response_format"] = rf.clone();
        }
        body
    }

    fn parse_response(&self, raw: &serde_json::Value) -> Result<UnifiedResponse, String> {
        let content = raw
            .get("content")
            .and_then(|c| c.as_array())
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
        // Anthropic: the full response object is the assistant message (with role added)
        let mut msg = raw.clone();
        if let Some(obj) = msg.as_object_mut() {
            obj.insert("role".to_string(), serde_json::json!("assistant"));
        }
        Ok(msg)
    }
}
