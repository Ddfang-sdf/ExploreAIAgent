use super::api_adapter::ApiAdapter;
use super::types::*;

impl ApiAdapter {
    pub fn build_tool_result_message(
        &self,
        tool_call_id: &str,
        content: &str,
    ) -> serde_json::Value {
        match self.api_mode {
            ApiMode::Chat => serde_json::json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "content": content,
            }),
            ApiMode::Responses => serde_json::json!({
                "type": "function_call_output",
                "call_id": tool_call_id,
                "output": content,
            }),
        }
    }

    pub fn build_structured_output_constraint(
        &self,
        schema: &serde_json::Value,
    ) -> serde_json::Value {
        let name = schema
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("structured_output");
        let strict = schema
            .get("strict")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let inner_schema = schema
            .get("schema")
            .cloned()
            .unwrap_or_else(|| schema.clone());

        match self.api_mode {
            ApiMode::Chat => serde_json::json!({
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": name,
                        "strict": strict,
                        "schema": inner_schema,
                    }
                }
            }),
            ApiMode::Responses => serde_json::json!({
                "text": {
                    "format": {
                        "type": "json_schema",
                        "name": name,
                        "strict": strict,
                        "schema": inner_schema,
                    }
                }
            }),
        }
    }

    pub fn get_tool_call_format_description(&self) -> &str {
        match self.api_mode {
            ApiMode::Chat => CHAT_TOOL_CALL_FORMAT,
            ApiMode::Responses => RESPONSES_TOOL_CALL_FORMAT,
        }
    }
}
