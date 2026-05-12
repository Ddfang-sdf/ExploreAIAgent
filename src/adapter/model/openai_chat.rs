use super::ModelAdapter;
use crate::adapter::types::{ToolDefinition, UnifiedResponse, ToolCallInfo};
use crate::adapter::reasoning::ReasoningChain;

/// OpenAI Chat Completions API adapter.
///
/// Supports any model that implements the OpenAI `/v1/chat/completions` protocol,
/// including standard OpenAI, MiniMax, DeepSeek, and other compatible providers.
///
/// Model-specific quirks are expressed as optional configuration on the adapter
/// rather than separate adapter implementations.
pub struct OpenAiChatAdapter {
    pub id: String,

    // --- MiniMax-specific extensions ---
    /// MiniMax `thinking` field: `{"type": "enabled"|"disabled"}`
    pub thinking: Option<bool>,
    /// MiniMax `reasoning_split`: separate reasoning into `reasoning_details`
    pub reasoning_split: Option<bool>,
}

impl OpenAiChatAdapter {
    pub fn new(id: impl Into<String>) -> Self {
        OpenAiChatAdapter {
            id: id.into(),
            thinking: None,
            reasoning_split: None,
        }
    }

    pub fn with_thinking(mut self, enabled: bool) -> Self {
        self.thinking = Some(enabled);
        self
    }

    pub fn with_reasoning_split(mut self, enabled: bool) -> Self {
        self.reasoning_split = Some(enabled);
        self
    }
}

impl ModelAdapter for OpenAiChatAdapter {
    fn adapter_id(&self) -> &str {
        &self.id
    }

    fn format_tools(&self, tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        tools.iter().map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
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
            "messages": messages,
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools);
        }
        if let Some(rf) = response_format {
            body["response_format"] = rf.clone();
        }
        // MiniMax-specific
        if let Some(thinking) = self.thinking {
            body["thinking"] = serde_json::json!({
                "type": if thinking { "enabled" } else { "disabled" }
            });
        }
        if let Some(rs) = self.reasoning_split {
            body["reasoning_split"] = serde_json::json!(rs);
        }
        body
    }

    fn parse_response(&self, raw: &serde_json::Value) -> Result<UnifiedResponse, String> {
        let message = raw
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .ok_or_else(|| "Missing choices[0].message in response".to_string())?;

        let mut tool_calls = Vec::new();
        if let Some(tc_array) = message.get("tool_calls").and_then(|t| t.as_array()) {
            for tc in tc_array {
                let func = match tc.get("function") {
                    Some(f) => f,
                    None => continue,
                };
                let name = match func.get("name").and_then(|n| n.as_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let args_str = match func.get("arguments").and_then(|a| a.as_str()) {
                    Some(a) => a,
                    None => continue,
                };
                let id = tc.get("id").and_then(|i| i.as_str()).map(String::from);
                let arguments: serde_json::Value = serde_json::from_str(args_str).unwrap_or(serde_json::Value::Null);
                tool_calls.push(ToolCallInfo { id, name, arguments });
            }
        }

        let mut text: Option<String> = None;
        if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
            if !content.is_empty() {
                text = Some(content.to_string());
            }
        }

        let chain = ReasoningChain::default_chain();
        let (reasoning, text) = chain.extract(raw, text.as_deref());

        Ok(UnifiedResponse { text, tool_calls, reasoning })
    }

    fn build_assistant_message(&self, raw: &serde_json::Value) -> Result<serde_json::Value, String> {
        raw.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message").cloned())
            .ok_or_else(|| "Missing choices[0].message in response".to_string())
    }
}
